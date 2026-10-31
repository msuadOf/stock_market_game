//! 配股／增发的日终纯候选。所有 paid/refunded 数值都表示待提交事实，不能冒充组端执行回执。

use super::{QueuedRightsSubscription, RejectedRightsSubscription, SessionCorporateActions};
use crate::account::{AccountKind, StockCode};
use crate::calendar::CivilDate;
use crate::calendar::TradingCalendar;
use crate::company::cash_dividend_tax::CashDividendTaxBook;
use crate::company::rights_offering::{
    DirectedPlacementTarget, RightsOfferingError, RightsOfferingEventPlan, RightsOfferingMode,
};
use crate::company::rights_offering::{
    HolderRightsSettlement, RightsOfferingBook, RightsOfferingStatus, RightsSettlementReceipt,
    RightsSubscriptionRecord, SubscriptionOrigin,
};
use crate::company::share_registry::{
    AcquisitionSource, DayNetChange, HolderId, MovementScope, NetAcquisition, ShareDayRequest,
    ShareRegistry, ShareRestriction,
};
use crate::company::CompanyId;
use crate::money::Money;
use crate::orderbook::AccountId;
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

/// 真实日终组账户资源；available_cash 已扣在途占用，不得使用镜像总现金代替。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RightsFinalResources {
    pub available_cash: Money,
    pub kind: AccountKind,
}

/// 现有队列的一账户一净申请身份；不得把受理成功解释成缴款完成。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RightsRequestKey {
    pub event_id: String,
    pub account: AccountId,
    pub submitted_on: CivilDate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RightsAccountDisposition {
    PreparedSubscription,
    PreparedSettlement,
    Rejected { cause: RightsOfferingError },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RightsAccountResult {
    pub request: RightsRequestKey,
    pub origin: SubscriptionOrigin,
    pub requested_shares: u64,
    pub paid_shares: u64,
    pub waived_shares: u64,
    pub paid_amount: Money,
    pub refunded_amount: Money,
    pub disposition: RightsAccountDisposition,
}

/// 独立的配股领域意图。D1a 既有股利／税款操作不能替代这些语义。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RightsOfferingIntent {
    PaymentDebit {
        request: RightsRequestKey,
        amount: Money,
    },
    FailureRefund {
        request: RightsRequestKey,
        amount: Money,
    },
    /// paid_amount 是逐认购实缴成本；后续 SharesWithCost 必须增加 invested，不能再次扣 cash 或佣金。
    NewShares {
        request: RightsRequestKey,
        stock: StockCode,
        shares: u64,
        paid_amount: Money,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RightsFinanceSettlement {
    pub issuer: CompanyId,
    pub event_id: String,
    pub settled_on: CivilDate,
    pub issued_shares: u64,
    pub proceeds: Money,
}

/// 仅本簇会变化的候选字段；调用方在全部组回执勾稽后才可安装。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RightsOfferingDayEndCandidate {
    pub registries: Vec<ShareRegistry>,
    pub dividend_tax_books: Vec<CashDividendTaxBook>,
    pub rights_offerings: Vec<RightsOfferingBook>,
    pub rights_subscription_queue: Vec<QueuedRightsSubscription>,
    pub rejected_rights_subscriptions: Vec<RejectedRightsSubscription>,
    pub results: Vec<RightsAccountResult>,
    pub intents: Vec<RightsOfferingIntent>,
    pub finance_settlements: Vec<RightsFinanceSettlement>,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub(crate) enum RightsOfferingCandidateError {
    #[error("配股事件 {event_id} 尚不支持长期限售，lock_until={lock_until}")]
    UnsupportedLock {
        event_id: String,
        lock_until: CivilDate,
    },
    #[error("配股事件 {event_id} 在 {day} 缺少账户 {account:?} 的最终可用资源")]
    MissingAccount {
        event_id: String,
        account: AccountId,
        day: CivilDate,
    },
    #[error("配股事件 {event_id} 在 {day} 的账户 {account:?} 可用现金为负：{cash:?}")]
    NegativeCash {
        event_id: String,
        account: AccountId,
        day: CivilDate,
        cash: Money,
    },
    #[error("配股事件 {event_id} 在 {day} 的领域校验失败：{source}")]
    Domain {
        event_id: String,
        day: CivilDate,
        source: RightsOfferingError,
    },
    #[error("配股事件 {event_id} 在 {day} 的候选状态错误：{detail}")]
    Invalid {
        event_id: String,
        day: CivilDate,
        detail: String,
    },
}

/// Session 命令和恢复接线必须复用的能力门禁；不删除纯公司领域表达限售的能力。
pub(crate) fn validate_rights_offering_capability(
    plan: &RightsOfferingEventPlan,
) -> Result<(), RightsOfferingCandidateError> {
    if let RightsOfferingMode::DirectedPlacement { targets } = &plan.mode {
        for target in targets {
            if let DirectedPlacementTarget::NamedHolder {
                lock_until: Some(lock_until),
                ..
            } = target
            {
                return Err(RightsOfferingCandidateError::UnsupportedLock {
                    event_id: plan.event_id.clone(),
                    lock_until: *lock_until,
                });
            }
        }
    }
    Ok(())
}

fn invalid(
    plan: &RightsOfferingEventPlan,
    day: CivilDate,
    detail: impl Into<String>,
) -> RightsOfferingCandidateError {
    RightsOfferingCandidateError::Invalid {
        event_id: plan.event_id.clone(),
        day,
        detail: detail.into(),
    }
}
fn domain(
    plan: &RightsOfferingEventPlan,
    day: CivilDate,
    source: RightsOfferingError,
) -> RightsOfferingCandidateError {
    RightsOfferingCandidateError::Domain {
        event_id: plan.event_id.clone(),
        day,
        source,
    }
}
fn request(
    plan: &RightsOfferingEventPlan,
    account: AccountId,
    submitted_on: CivilDate,
) -> RightsRequestKey {
    RightsRequestKey {
        event_id: plan.event_id.clone(),
        account,
        submitted_on,
    }
}
fn available(
    balances: &BTreeMap<AccountId, RightsFinalResources>,
    plan: &RightsOfferingEventPlan,
    day: CivilDate,
    account: AccountId,
) -> Result<RightsFinalResources, RightsOfferingCandidateError> {
    let resource =
        *balances
            .get(&account)
            .ok_or_else(|| RightsOfferingCandidateError::MissingAccount {
                event_id: plan.event_id.clone(),
                day,
                account,
            })?;
    if resource.available_cash < Money::ZERO {
        return Err(RightsOfferingCandidateError::NegativeCash {
            event_id: plan.event_id.clone(),
            day,
            account,
            cash: resource.available_cash,
        });
    }
    Ok(resource)
}

/// 在最终市场资源上计算；personal_tax_accounts 必须来自 AShareIndividual 的个人资格集。
/// 同一账户跨事件按 rights_offerings 的冻结顺序共享余额；不修改任何调用方对象。
pub(crate) fn rights_offering_day_end_candidate(
    actions: &SessionCorporateActions,
    day: CivilDate,
    calendar: &TradingCalendar,
    resources: &BTreeMap<AccountId, RightsFinalResources>,
    personal_tax_accounts: &BTreeSet<AccountId>,
) -> Result<RightsOfferingDayEndCandidate, RightsOfferingCandidateError> {
    let mut candidate = RightsOfferingDayEndCandidate {
        registries: actions.registries.clone(),
        dividend_tax_books: actions.dividend_tax_books.clone(),
        rights_offerings: actions.rights_offerings.clone(),
        rights_subscription_queue: actions.rights_subscription_queue.clone(),
        rejected_rights_subscriptions: actions.rejected_rights_subscriptions.clone(),
        results: vec![],
        intents: vec![],
        finance_settlements: vec![],
    };
    let mut balances = resources.clone();
    let mut events = BTreeSet::new();
    for book in &candidate.rights_offerings {
        let plan = book.plan();
        validate_rights_offering_capability(plan)?;
        book.validate_with_calendar(calendar)
            .map_err(|error| domain(plan, day, error))?;
        if !events.insert(plan.event_id.clone()) {
            return Err(invalid(plan, day, "配股事件身份重复"));
        }
    }
    let mut queued_keys = BTreeSet::new();
    for queued in &candidate.rights_subscription_queue {
        if !queued_keys.insert((queued.event_id.clone(), queued.account, queued.submitted_on)) {
            return Err(RightsOfferingCandidateError::Invalid {
                event_id: queued.event_id.clone(),
                day,
                detail: "认购队列申请身份重复，无法关联唯一缴款结果".into(),
            });
        }
        let book = candidate
            .rights_offerings
            .iter()
            .find(|book| book.plan().event_id == queued.event_id)
            .ok_or_else(|| RightsOfferingCandidateError::Invalid {
                event_id: queued.event_id.clone(),
                day,
                detail: "认购队列找不到事件".into(),
            })?;
        if !book.plan().payment_window_contains(queued.submitted_on) || queued.submitted_on > day {
            return Err(invalid(
                book.plan(),
                day,
                "认购队列提交日期不在已到达的缴款窗口",
            ));
        }
        if book.status() != &RightsOfferingStatus::Entitled || day > book.plan().payment_deadline_on
        {
            return Err(invalid(book.plan(), day, "认购队列不能在已关窗事件中遗留"));
        }
    }
    for index in 0..candidate.rights_offerings.len() {
        let plan = candidate.rights_offerings[index].plan().clone();
        if candidate.rights_offerings[index].status() == &RightsOfferingStatus::Approved {
            if day > plan.announced_on {
                return Err(invalid(&plan, day, "错过公告日"));
            }
            if day == plan.announced_on {
                candidate.rights_offerings[index]
                    .announce(day)
                    .map_err(|error| domain(&plan, day, error))?;
            }
        }
        if day == plan.registered_on && candidate.rights_offerings[index].registration().is_none() {
            let registry = candidate
                .registries
                .iter_mut()
                .find(|registry| {
                    registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                })
                .ok_or_else(|| invalid(&plan, day, "登记日缺少匹配名册"))?;
            let snapshot = registry
                .register(plan.event_id.clone(), day)
                .map_err(|error| domain(&plan, day, error.into()))?
                .clone();
            candidate.rights_offerings[index]
                .entitle(snapshot, calendar)
                .map_err(|error| domain(&plan, day, error))?;
        } else if day > plan.registered_on
            && candidate.rights_offerings[index].registration().is_none()
        {
            return Err(invalid(&plan, day, "错过登记日"));
        }
        if candidate.rights_offerings[index].status() == &RightsOfferingStatus::Entitled
            && plan.payment_window_contains(day)
        {
            // 已受理的显式申请先于默认策略；NPC 显式认购后不再被默认策略覆盖。
            let mut pending = Vec::new();
            candidate.rights_subscription_queue.retain(|queued| {
                if queued.event_id == plan.event_id {
                    pending.push(queued.clone());
                    false
                } else {
                    true
                }
            });
            let explicit_accounts: BTreeSet<AccountId> =
                pending.iter().map(|queued| queued.account).collect();
            for queued in pending {
                prepare_subscription(
                    &mut candidate,
                    index,
                    day,
                    queued.account,
                    queued.submitted_on,
                    queued.requested_shares,
                    SubscriptionOrigin::Explicit,
                    &mut balances,
                )?;
            }
            if day == plan.payment_start_on {
                let entitlements = candidate.rights_offerings[index]
                    .entitlement()
                    .ok_or_else(|| invalid(&plan, day, "缺少权证回执"))?
                    .entitlements
                    .clone();
                for entitlement in entitlements {
                    if let HolderId::Account(account) = entitlement.holder {
                        let resource = available(&balances, &plan, day, account)?;
                        let subscribed = candidate.rights_offerings[index]
                            .subscriptions()
                            .iter()
                            .any(|record| record.holder == HolderId::Account(account));
                        if entitlement.rights_shares > 0
                            && !subscribed
                            && !explicit_accounts.contains(&account)
                            // 拒绝事实已随候选安装／恢复保留，同日重试不能自动替换该显式意图。
                            && !candidate.rejected_rights_subscriptions.iter().any(|rejected| {
                                rejected.event_id == plan.event_id
                                    && rejected.account == account
                                    && rejected.rejected_on == day
                            })
                            && !matches!(
                                resource.kind,
                                AccountKind::Player | AccountKind::IssuerRepurchase
                            )
                        {
                            prepare_subscription(
                                &mut candidate,
                                index,
                                day,
                                account,
                                day,
                                entitlement.rights_shares,
                                SubscriptionOrigin::NpcFullByDefault,
                                &mut balances,
                            )?;
                        }
                    }
                }
            }
        }
        if day == plan.payment_deadline_on
            && candidate.rights_offerings[index].status() == &RightsOfferingStatus::Entitled
        {
            candidate.rights_offerings[index]
                .close_payment_window(day)
                .map_err(|error| domain(&plan, day, error))?;
        } else if day > plan.payment_deadline_on
            && candidate.rights_offerings[index].status() == &RightsOfferingStatus::Entitled
        {
            return Err(invalid(&plan, day, "错过缴款截止关窗"));
        }
        if day == plan.settlement_on
            && candidate.rights_offerings[index].status() == &RightsOfferingStatus::Closed
        {
            prepare_settlement(
                &mut candidate,
                index,
                day,
                &mut balances,
                personal_tax_accounts,
            )?;
        } else if day > plan.settlement_on
            && candidate.rights_offerings[index].status() == &RightsOfferingStatus::Closed
        {
            return Err(invalid(&plan, day, "错过结算日"));
        }
    }
    Ok(candidate)
}

#[allow(clippy::too_many_arguments)]
fn prepare_subscription(
    candidate: &mut RightsOfferingDayEndCandidate,
    index: usize,
    day: CivilDate,
    account: AccountId,
    submitted_on: CivilDate,
    requested_shares: u64,
    origin: SubscriptionOrigin,
    balances: &mut BTreeMap<AccountId, RightsFinalResources>,
) -> Result<(), RightsOfferingCandidateError> {
    let plan = candidate.rights_offerings[index].plan().clone();
    let resource = available(balances, &plan, day, account)?;
    if resource.kind == AccountKind::IssuerRepurchase {
        return Err(invalid(&plan, day, "回购专户不能认购新股"));
    }
    let affordable = u64::try_from(resource.available_cash.cents() / plan.price_per_share.cents())
        .map_err(|_| invalid(&plan, day, "可认购数量溢出"))?;
    let paid_shares = requested_shares.min(affordable);
    let paid_amount = Money::from_cents(
        i64::try_from(
            u128::from(paid_shares) * u128::from(plan.price_per_share.cents().unsigned_abs()),
        )
        .map_err(|_| invalid(&plan, day, "缴款金额溢出"))?,
    );
    let record = RightsSubscriptionRecord {
        holder: HolderId::Account(account),
        requested_shares,
        price_per_share: plan.price_per_share,
        submitted_on,
        origin: origin.clone(),
        paid_shares,
        paid_amount,
        waived_shares: requested_shares - paid_shares,
    };
    let key = request(&plan, account, submitted_on);
    if let Err(cause) = candidate.rights_offerings[index].record_subscription(record.clone()) {
        candidate
            .rejected_rights_subscriptions
            .push(RejectedRightsSubscription {
                event_id: plan.event_id,
                account,
                requested_shares,
                submitted_on,
                rejected_on: day,
                reason: cause.to_string(),
            });
        candidate.results.push(RightsAccountResult {
            request: key,
            origin,
            requested_shares,
            paid_shares: 0,
            waived_shares: 0,
            paid_amount: Money::ZERO,
            refunded_amount: Money::ZERO,
            disposition: RightsAccountDisposition::Rejected { cause },
        });
        return Ok(());
    }
    let remaining = resource
        .available_cash
        .sub(paid_amount)
        .map_err(|error| invalid(&plan, day, error.to_string()))?;
    balances.insert(
        account,
        RightsFinalResources {
            available_cash: remaining,
            ..resource
        },
    );
    candidate.results.push(RightsAccountResult {
        request: key.clone(),
        origin,
        requested_shares,
        paid_shares,
        waived_shares: record.waived_shares,
        paid_amount,
        refunded_amount: Money::ZERO,
        disposition: RightsAccountDisposition::PreparedSubscription,
    });
    if paid_shares > 0 {
        candidate.intents.push(RightsOfferingIntent::PaymentDebit {
            request: key,
            amount: paid_amount,
        });
    }
    Ok(())
}

fn prepare_settlement(
    candidate: &mut RightsOfferingDayEndCandidate,
    index: usize,
    day: CivilDate,
    balances: &mut BTreeMap<AccountId, RightsFinalResources>,
    personal_tax_accounts: &BTreeSet<AccountId>,
) -> Result<(), RightsOfferingCandidateError> {
    let plan = candidate.rights_offerings[index].plan().clone();
    let failed = candidate.rights_offerings[index]
        .determine_failure()
        .map_err(|error| domain(&plan, day, error))?;
    let subscriptions = candidate.rights_offerings[index].subscriptions().to_vec();
    let mut holders = Vec::new();
    let mut total_paid_shares = 0_u64;
    let mut total_paid_amount = Money::ZERO;
    let mut refunded_total = Money::ZERO;
    let mut changes = Vec::new();
    for record in &subscriptions {
        let refunded_amount = if failed {
            record.paid_amount
        } else {
            Money::ZERO
        };
        total_paid_shares = total_paid_shares
            .checked_add(record.paid_shares)
            .ok_or_else(|| invalid(&plan, day, "认购合计溢出"))?;
        total_paid_amount = total_paid_amount
            .add(record.paid_amount)
            .map_err(|error| invalid(&plan, day, error.to_string()))?;
        refunded_total = refunded_total
            .add(refunded_amount)
            .map_err(|error| invalid(&plan, day, error.to_string()))?;
        holders.push(HolderRightsSettlement {
            holder: record.holder.clone(),
            paid_shares: record.paid_shares,
            paid_amount: record.paid_amount,
            waived_shares: record.waived_shares,
            refunded_amount,
        });
        if let HolderId::Account(account) = record.holder {
            let resource = available(balances, &plan, day, account)?;
            let key = request(&plan, account, record.submitted_on);
            candidate.results.push(RightsAccountResult {
                request: key.clone(),
                origin: record.origin.clone(),
                requested_shares: record.requested_shares,
                paid_shares: record.paid_shares,
                waived_shares: record.waived_shares,
                paid_amount: record.paid_amount,
                refunded_amount,
                disposition: RightsAccountDisposition::PreparedSettlement,
            });
            if failed && refunded_amount > Money::ZERO {
                let cash = resource
                    .available_cash
                    .add(refunded_amount)
                    .map_err(|error| invalid(&plan, day, error.to_string()))?;
                balances.insert(
                    account,
                    RightsFinalResources {
                        available_cash: cash,
                        ..resource
                    },
                );
                candidate.intents.push(RightsOfferingIntent::FailureRefund {
                    request: key,
                    amount: refunded_amount,
                });
            } else if !failed && record.paid_shares > 0 {
                candidate.intents.push(RightsOfferingIntent::NewShares {
                    request: key,
                    stock: plan.stock.clone(),
                    shares: record.paid_shares,
                    paid_amount: record.paid_amount,
                });
                prepare_tax_credit(
                    candidate,
                    &plan,
                    day,
                    account,
                    record.paid_shares,
                    personal_tax_accounts,
                )?;
            }
        }
        if !failed && record.paid_shares > 0 {
            let holder_key = match &record.holder {
                HolderId::Account(account) => format!("account-{}", account.0),
                HolderId::External(name) => format!("external-{name}"),
                HolderId::IssuerTreasury => {
                    return Err(invalid(&plan, day, "回购专户不能取得配股新股"))
                }
            };
            changes.push(DayNetChange {
                holder: record.holder.clone(),
                change: i128::from(record.paid_shares),
                acquisition: Some(NetAcquisition {
                    lot_id: format!("rights-offering:{}:{holder_key}", plan.event_id),
                    source: AcquisitionSource::CorporateAction {
                        event: plan.event_id.clone(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }),
                consume: None,
            });
        }
    }
    if !changes.is_empty() {
        let registry = candidate
            .registries
            .iter_mut()
            .find(|registry| registry.stock() == &plan.stock && registry.issuer() == &plan.issuer)
            .ok_or_else(|| invalid(&plan, day, "新股入账缺少匹配名册"))?;
        registry
            .close_day(ShareDayRequest {
                event_id: format!("rights-offering:{}", plan.event_id),
                day,
                scope: MovementScope::NonTradingTransfer {
                    basis: plan.approval_reference.clone(),
                },
                changes,
            })
            .map_err(|error| domain(&plan, day, error.into()))?;
    }
    candidate.finance_settlements.push(RightsFinanceSettlement {
        issuer: plan.issuer,
        event_id: plan.event_id.clone(),
        settled_on: day,
        issued_shares: if failed { 0 } else { total_paid_shares },
        proceeds: if failed {
            Money::ZERO
        } else {
            total_paid_amount
        },
    });
    candidate.rights_offerings[index]
        .settle(
            day,
            RightsSettlementReceipt {
                event_id: plan.event_id.clone(),
                settlement_on: day,
                failed,
                total_paid_shares,
                total_paid_amount,
                refunded_total,
                holders,
            },
        )
        .map_err(|error| domain(&plan, day, error))?;
    if !failed {
        candidate.rights_offerings[index]
            .mark_credited(day)
            .map_err(|error| domain(&plan, day, error))?;
    }
    Ok(())
}

fn prepare_tax_credit(
    candidate: &mut RightsOfferingDayEndCandidate,
    plan: &RightsOfferingEventPlan,
    day: CivilDate,
    account: AccountId,
    shares: u64,
    personal_tax_accounts: &BTreeSet<AccountId>,
) -> Result<(), RightsOfferingCandidateError> {
    use crate::company::cash_dividend_tax::{
        convert_tax_source, DividendTaxLot, DividendTaxProfile, TaxShareClass,
    };
    if !candidate
        .dividend_tax_books
        .iter()
        .any(|book| book.account() == account && book.stock() == &plan.stock)
        && personal_tax_accounts.contains(&account)
    {
        let registry = candidate
            .registries
            .iter()
            .find(|registry| registry.stock() == &plan.stock && registry.issuer() == &plan.issuer)
            .ok_or_else(|| invalid(plan, day, "首次个人税账开账缺少匹配名册"))?;
        if registry.holdings().iter().any(|holding| {
            holding.holder == HolderId::Account(account)
                && holding.lots.iter().any(|lot| lot.qty > 0)
        }) {
            return Err(invalid(
                plan,
                day,
                format!("个人账户 {account:?} 已持股但缺少日终税账，不能以空基线掩盖"),
            ));
        }
        let previous = day
            .prev()
            .map_err(|error| invalid(plan, day, error.to_string()))?;
        candidate.dividend_tax_books.push(
            CashDividendTaxBook::new(
                account,
                plan.stock.clone(),
                DividendTaxProfile::IndividualPublicMarket,
                previous,
                vec![],
            )
            .map_err(|error| invalid(plan, day, error.to_string()))?,
        );
    }
    if let Some(book) = candidate
        .dividend_tax_books
        .iter_mut()
        .find(|book| book.account() == account && book.stock() == &plan.stock)
    {
        book.record_net_day(
            format!("rights-offering:{}:{}", plan.event_id, account.0),
            day,
            i128::from(shares),
            Some(DividendTaxLot {
                id: format!("tax:rights-offering:{}:{}", plan.event_id, account.0),
                qty: shares,
                acquired_on: day,
                source: convert_tax_source(&AcquisitionSource::CorporateAction {
                    event: plan.event_id.clone(),
                }),
                class: TaxShareClass::PublicMarket,
            }),
        )
        .map_err(|error| invalid(plan, day, error.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "rights_offering_day_end_tests.rs"]
mod tests;
