//! 消费已冻结的重新计值事实，生成 PreNewUnitMarket 候选；不执行真实账户入账。
//!
//! 本模块不生成碎股政策、不表示普通 A 股拆缩股产品已通过制度审核。
//! 新单位成交必须在此候选之后另行处理，不能先混入原单位 registry。

use crate::account::StockCode;
use crate::calendar::CivilDate;
use crate::company::cash_dividend_tax::CashDividendTaxBook;
use crate::company::issuer_repurchase::SplitUnitContext;
use crate::company::share_registry::ShareRegistry;
use crate::company::share_split::ShareSplitBook;
use crate::orderbook::AccountId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TreasuryBucketTransform {
    pub key: String,
    pub original_lot_ids: Vec<String>,
    pub shares_before: u64,
    pub shares_after: u64,
    /// initial 库存不自动归属真实回购账户；真实 plan 桶须显式关联。
    pub account: Option<AccountId>,
}

pub(crate) struct ShareTransformDayEndInput<'a> {
    pub day: CivilDate,
    pub book: &'a ShareSplitBook,
    pub registry_in_registered_units: &'a ShareRegistry,
    pub dividend_tax_books: &'a [CashDividendTaxBook],
    pub unit_chain: &'a [SplitUnitContext],
    pub treasury_buckets: &'a [TreasuryBucketTransform],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ShareTransformAmount {
    ShareCredit { qty: u64 },
    ShareReduction { qty: u64 },
}

/// 成本累计事实保留；只有实际账户最终零仓才清除成本，不能以登记份额为零推定。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ShareTransformCostPolicy {
    PreserveUnlessActualPositionCloses,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountShareTransformIntent {
    pub event_id: String,
    pub account: AccountId,
    pub stock: StockCode,
    pub source_bucket: Option<String>,
    pub registered_before: u64,
    pub converted_after: u64,
    pub amount: ShareTransformAmount,
    pub cost_policy: ShareTransformCostPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ShareTransformCandidatePhase {
    PreNewUnitMarket,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShareTransformDayEndCandidate {
    pub phase: ShareTransformCandidatePhase,
    pub stock: StockCode,
    pub unit_event_id: String,
    pub registry_before_new_unit_market: ShareRegistry,
    pub dividend_tax_books_before_new_unit_market: Vec<CashDividendTaxBook>,
    pub settled_book: ShareSplitBook,
    pub account_intents: Vec<AccountShareTransformIntent>,
    pub treasury_adjustments: Vec<TreasuryBucketTransform>,
}

#[derive(Debug, thiserror::Error)]
#[error("重新计值候选 {event_id} 在 {day} 失败：{detail}")]
pub(crate) struct ShareTransformDayEndError {
    pub event_id: String,
    pub day: CivilDate,
    pub detail: String,
}

pub(crate) fn share_transform_day_end_candidate(
    input: ShareTransformDayEndInput<'_>,
) -> Result<ShareTransformDayEndCandidate, ShareTransformDayEndError> {
    use crate::company::cash_dividend_tax::{
        convert_tax_class, convert_tax_source, DividendTaxLot,
    };
    use crate::company::share_registry::{
        AcquisitionSource, HolderId, MovementScope, ShareDayRequest,
    };
    use crate::company::share_split::{holder_inherited_restriction, ShareSplitStatus};
    use std::collections::BTreeSet;

    let plan = input.book.plan();
    let fail = |detail: String| ShareTransformDayEndError {
        event_id: plan.event_id.clone(),
        day: input.day,
        detail,
    };
    input
        .book
        .validate()
        .map_err(|error| fail(error.to_string()))?;
    if input.day != plan.ex_rights_on || input.book.status() != &ShareSplitStatus::Registered {
        return Err(fail(
            "只有 ex 日的 Registered 冻结账簿可以生成入账候选".into(),
        ));
    }
    let registration = input
        .book
        .registration()
        .ok_or_else(|| fail("缺少 R 登记快照".into()))?;
    let receipt = input
        .book
        .receipt()
        .ok_or_else(|| fail("缺少冻结换算回执".into()))?;
    let registry = input.registry_in_registered_units;
    registry
        .validate()
        .map_err(|error| fail(error.to_string()))?;
    if registry.stock() != &plan.stock
        || registry.issuer() != &plan.issuer
        || registry.issued_shares() != receipt.issued_shares_before
        || registry.holdings() != registration.holdings()
        || registry.registration(&plan.event_id) != Some(registration)
        || !matches!(input.day.days_since(registry.settled_on()), 0 | 1)
        || registry.receipts().iter().any(|entry| {
            entry.request.day > plan.registered_on
                && matches!(entry.request.scope, MovementScope::PublicMarket)
                && !entry.request.changes.is_empty()
        })
    {
        return Err(fail(
            "原单位 registry 与 R 冻结事实不一致，或已经混入登记后市场变动".into(),
        ));
    }
    let mut unit_ids = BTreeSet::new();
    let mut previous_ex = None;
    for unit in input.unit_chain {
        if unit.event_id.trim().is_empty()
            || unit.ratio < 2
            || !unit_ids.insert(&unit.event_id)
            || previous_ex.is_some_and(|previous| previous >= unit.ex_on)
        {
            return Err(fail(
                "单位链必须身份唯一、比例有效并按实际 ex 日期严格递增".into(),
            ));
        }
        previous_ex = Some(unit.ex_on);
    }
    let current_unit = input
        .unit_chain
        .iter()
        .find(|unit| unit.event_id == plan.event_id)
        .ok_or_else(|| fail("单位链缺少当前冻结事件".into()))?;
    if current_unit.ex_on != input.day
        || current_unit.ratio != plan.ratio
        || current_unit.direction != plan.direction
    {
        return Err(fail(
            "单位链当前事件与冻结计划的日期、方向或比例不一致".into(),
        ));
    }
    crate::company::issuer_repurchase::unit_conversion_factor(None, input.day, input.unit_chain)
        .map_err(|error| fail(error.to_string()))?;

    let mut tax_identities = BTreeSet::new();
    for tax in input.dividend_tax_books {
        tax.validate().map_err(|error| fail(error.to_string()))?;
        if !tax_identities.insert((tax.account(), tax.stock())) {
            return Err(fail("重复的账户证券税账".into()));
        }
        if tax.stock() != &plan.stock {
            continue;
        }
        let expected = receipt
            .holders
            .iter()
            .find(|outcome| outcome.holder == HolderId::Account(tax.account()))
            .map_or(0, |outcome| outcome.original_shares);
        let actual = checked_total(tax.lots().iter().map(|lot| lot.qty)).map_err(&fail)?;
        if actual != expected {
            return Err(fail(format!(
                "账户 {:?} 的原单位税账数量 {actual} 与登记数量 {expected} 不一致",
                tax.account()
            )));
        }
    }
    let mut result = ShareTransformDayEndCandidate {
        phase: ShareTransformCandidatePhase::PreNewUnitMarket,
        stock: plan.stock.clone(),
        unit_event_id: plan.event_id.clone(),
        registry_before_new_unit_market: registry.clone(),
        dividend_tax_books_before_new_unit_market: input.dividend_tax_books.to_vec(),
        settled_book: input.book.clone(),
        account_intents: vec![],
        treasury_adjustments: vec![],
    };
    let mut changes = Vec::new();
    let mut has_treasury = false;
    for outcome in &receipt.holders {
        if outcome.holder == HolderId::IssuerTreasury {
            has_treasury = true;
            let mut bucket_keys = BTreeSet::new();
            let mut lot_ids = BTreeSet::new();
            for bucket in input.treasury_buckets {
                if bucket.key.trim().is_empty() || !bucket_keys.insert(&bucket.key) {
                    return Err(fail("Treasury 桶 key 为空或重复".into()));
                }
                // 仅验证显式结果的数学可行区间，不生成碎股排序或改写上游冻结结果。
                let (numerator, denominator) = plan.new_shares_per_old_share();
                let product = u128::from(bucket.shares_before) * u128::from(numerator);
                let minimum = u64::try_from(product / u128::from(denominator))
                    .map_err(|_| fail(format!("Treasury 桶 {} 换算数量溢出", bucket.key)))?;
                let maximum = minimum
                    .checked_add(u64::from(product % u128::from(denominator) > 0))
                    .ok_or_else(|| fail(format!("Treasury 桶 {} 换算上界溢出", bucket.key)))?;
                if !(minimum..=maximum).contains(&bucket.shares_after) {
                    return Err(fail(format!(
                        "Treasury 桶 {} 的显式结果 {} 不在换算可行区间 {minimum}..={maximum}",
                        bucket.key, bucket.shares_after
                    )));
                }
                let mut lots = Vec::new();
                for id in &bucket.original_lot_ids {
                    if !lot_ids.insert(id) {
                        return Err(fail(format!("Treasury lot {id} 重复归属")));
                    }
                    lots.push(
                        outcome
                            .original_lots
                            .iter()
                            .find(|lot| &lot.id == id)
                            .ok_or_else(|| {
                                fail(format!("Treasury 桶 {} 引用非登记 lot {id}", bucket.key))
                            })?
                            .clone(),
                    );
                }
                if checked_total(lots.iter().map(|lot| lot.qty)).map_err(&fail)?
                    != bucket.shares_before
                    || (bucket.shares_after > bucket.shares_before
                        && outcome.new_shares <= outcome.original_shares)
                    || (bucket.shares_after < bucket.shares_before
                        && outcome.new_shares >= outcome.original_shares)
                {
                    return Err(fail(format!(
                        "Treasury 桶 {} 数量或换算方向与冻结事实不符",
                        bucket.key
                    )));
                }
                if let Some(change) = transform_change(
                    &outcome.holder,
                    &lots,
                    bucket.shares_before,
                    bucket.shares_after,
                    input.day,
                    &plan.event_id,
                    Some(&bucket.key),
                )
                .map_err(&fail)?
                {
                    changes.push(change);
                }
                if let Some(account) = bucket.account {
                    push_intent(
                        &mut result.account_intents,
                        account,
                        &plan.event_id,
                        &plan.stock,
                        Some(bucket.key.clone()),
                        bucket.shares_before,
                        bucket.shares_after,
                    );
                }
            }
            if lot_ids.len() != outcome.original_lots.len()
                || checked_total(
                    input
                        .treasury_buckets
                        .iter()
                        .map(|bucket| bucket.shares_before),
                )
                .map_err(&fail)?
                    != outcome.original_shares
                || checked_total(
                    input
                        .treasury_buckets
                        .iter()
                        .map(|bucket| bucket.shares_after),
                )
                .map_err(&fail)?
                    != outcome.new_shares
            {
                return Err(fail(
                    "Treasury 桶必须完整覆盖登记 lots，且换算前后总量分别勾稽冻结回执".into(),
                ));
            }
            result.treasury_adjustments = input.treasury_buckets.to_vec();
            continue;
        }
        if let Some(change) = transform_change(
            &outcome.holder,
            &outcome.original_lots,
            outcome.original_shares,
            outcome.new_shares,
            input.day,
            &plan.event_id,
            None,
        )
        .map_err(&fail)?
        {
            changes.push(change);
        }
        if let HolderId::Account(account) = outcome.holder {
            push_intent(
                &mut result.account_intents,
                account,
                &plan.event_id,
                &plan.stock,
                None,
                outcome.original_shares,
                outcome.new_shares,
            );
            if outcome.new_shares == outcome.original_shares {
                continue;
            }
            let Some(tax) = result
                .dividend_tax_books_before_new_unit_market
                .iter_mut()
                .find(|tax| tax.stock() == &plan.stock && tax.account() == account)
            else {
                continue;
            };
            let tax_event = format!(
                "share-transform-tax:{}",
                serde_json::to_string(&(&plan.event_id, account))
                    .map_err(|error| fail(error.to_string()))?
            );
            if outcome.new_shares > outcome.original_shares {
                let restriction = holder_inherited_restriction(outcome, input.day)
                    .map_err(|error| fail(error.to_string()))?;
                let qty = outcome.new_shares - outcome.original_shares;
                tax.record_net_day(
                    tax_event.clone(),
                    input.day,
                    i128::from(qty),
                    Some(DividendTaxLot {
                        id: tax_event,
                        qty,
                        acquired_on: input.day,
                        source: convert_tax_source(&AcquisitionSource::CorporateAction {
                            event: plan.event_id.clone(),
                        }),
                        class: convert_tax_class(&restriction),
                    }),
                )
                .map_err(|error| fail(error.to_string()))?;
            } else {
                tax.record_redenomination_reduction(
                    tax_event,
                    input.day,
                    outcome.original_shares - outcome.new_shares,
                )
                .map_err(|error| fail(error.to_string()))?;
            }
        }
    }
    if !has_treasury && !input.treasury_buckets.is_empty() {
        return Err(fail("无登记 Treasury 库存却传入转换桶".into()));
    }
    result
        .registry_before_new_unit_market
        .close_day(ShareDayRequest {
            event_id: format!("share-transform:{}", plan.event_id),
            day: input.day,
            scope: MovementScope::ShareReDenomination {
                basis: plan.approval_reference.clone(),
            },
            changes,
        })
        .map_err(|error| fail(error.to_string()))?;
    if result.registry_before_new_unit_market.issued_shares() != receipt.issued_shares_after {
        return Err(fail("转换后的发行总股数与冻结回执不一致".into()));
    }
    result
        .settled_book
        .mark_settled(input.day)
        .map_err(|error| fail(error.to_string()))?;
    Ok(result)
}

fn checked_total(mut values: impl Iterator<Item = u64>) -> Result<u64, String> {
    values
        .try_fold(0_u64, |sum, value| sum.checked_add(value))
        .ok_or_else(|| "股份数量合计溢出".into())
}

fn push_intent(
    intents: &mut Vec<AccountShareTransformIntent>,
    account: AccountId,
    event_id: &str,
    stock: &StockCode,
    source_bucket: Option<String>,
    before: u64,
    after: u64,
) {
    let amount = if after > before {
        ShareTransformAmount::ShareCredit {
            qty: after - before,
        }
    } else if after < before {
        ShareTransformAmount::ShareReduction {
            qty: before - after,
        }
    } else {
        return;
    };
    intents.push(AccountShareTransformIntent {
        event_id: event_id.into(),
        account,
        stock: stock.clone(),
        source_bucket,
        registered_before: before,
        converted_after: after,
        amount,
        cost_policy: ShareTransformCostPolicy::PreserveUnlessActualPositionCloses,
    });
}

fn transform_change(
    holder: &crate::company::share_registry::HolderId,
    lots: &[crate::company::share_registry::ShareLot],
    before: u64,
    after: u64,
    day: CivilDate,
    event_id: &str,
    bucket: Option<&str>,
) -> Result<Option<crate::company::share_registry::DayNetChange>, String> {
    use crate::company::share_registry::{
        AcquisitionSource, DayNetChange, LotConsumption, NetAcquisition,
    };
    use crate::company::share_split::{holder_inherited_restriction, HolderSplitOutcome};
    if after == before {
        return Ok(None);
    }
    if after > before {
        let outcome = HolderSplitOutcome {
            holder: holder.clone(),
            original_shares: before,
            new_shares: after,
            fractional_numerator: 0,
            tie_break_award: 0,
            original_lots: lots.to_vec(),
        };
        let restriction =
            holder_inherited_restriction(&outcome, day).map_err(|error| error.to_string())?;
        let identity = serde_json::to_string(&(event_id, holder, bucket))
            .map_err(|error| error.to_string())?;
        Ok(Some(DayNetChange {
            holder: holder.clone(),
            change: i128::from(after - before),
            acquisition: Some(NetAcquisition {
                lot_id: format!("share-transform:{identity}"),
                source: AcquisitionSource::CorporateAction {
                    event: event_id.into(),
                },
                restriction,
            }),
            consume: None,
        }))
    } else {
        let mut remaining = before - after;
        let mut consume = Vec::new();
        for lot in lots {
            let qty = remaining.min(lot.qty);
            if qty > 0 {
                consume.push(LotConsumption {
                    lot_id: lot.id.clone(),
                    qty,
                });
                remaining -= qty;
            }
        }
        if remaining != 0 {
            return Err("登记原 lot 不足以执行冻结核减".into());
        }
        Ok(Some(DayNetChange {
            holder: holder.clone(),
            change: -i128::from(before - after),
            acquisition: None,
            consume: Some(consume),
        }))
    }
}

#[cfg(test)]
#[path = "share_transform_day_end_tests.rs"]
mod tests;
