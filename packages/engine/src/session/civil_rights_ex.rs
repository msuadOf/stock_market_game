//! 配股除权由当日日终真实公司候选与交易部收盘共同准备。
use super::*;
use crate::company::cash_dividend_tax::CashDividendTaxMode;
use crate::company::ex_reference_price::{
    rights_offering_ex_rights_reference_price, RightsOfferingExRightsFormula,
};
use crate::company::rights_offering::{RightsOfferingMode, RightsOfferingStatus};
use crate::company::share_registry::{HolderId, RegistrationSnapshot, ShareRestriction};
use crate::runtime::DeskExRightsRequest;
use crate::session::corporate_actions::AppliedExReferenceGroup;

pub(in crate::session) fn all_shares_circulating(snapshot: &RegistrationSnapshot) -> bool {
    let mut total = 0u64;
    for holding in snapshot.holdings() {
        if holding.holder == HolderId::IssuerTreasury {
            return false;
        }
        for lot in &holding.lots {
            if lot.restriction != ShareRestriction::Unrestricted {
                return false;
            }
            let Some(next) = total.checked_add(lot.qty) else {
                return false;
            };
            total = next;
        }
    }
    total == snapshot.issued_shares() && total > 0
}

impl GameSession {
    /// 串行 civil shadow 使用同一除权候选，并在投影前校验、安装真实市场基准。
    pub(super) fn prepare_serial_rights_ex_anchors(
        &mut self,
        next_date: CivilDate,
    ) -> Result<(), SessionError> {
        let requests = self.prepare_rights_ex_anchors(next_date)?;
        for request in requests {
            let stock = StockId(u32::from(request.stock));
            let market = self.state.markets.get_mut(&stock).ok_or_else(|| {
                SessionError::UnsupportedCapitalBoundary(format!("除权缺少证券 {stock:?} 的市场"))
            })?;
            market
                .install_ex_reference(
                    Money::from_cents(request.expected_last_close),
                    Money::from_cents(request.reference_price),
                )
                .map_err(|error| {
                    SessionError::UnsupportedCapitalBoundary(format!(
                        "证券 {stock:?} 的除权参考价校验失败：{error}"
                    ))
                })?;
        }
        Ok(())
    }

    /// 盘前送转只开放能从 Drain 后完整基线逐户勾稽的范围。
    pub(super) fn validate_stock_opening_scope(
        &self,
        next: CivilDate,
        baseline: &super::civil_cash_distributions::CivilSettlementView,
    ) -> Result<bool, crate::runtime::RuntimeError> {
        use crate::company::stock_distribution::StockDistributionStatus;
        let Some(actions) = self.state.companies.corporate_actions() else {
            return Ok(false);
        };
        let due: Vec<_> = actions
            .stock_distributions
            .iter()
            .filter(|book| {
                book.status() != &StockDistributionStatus::Credited
                    && book.plan().ex_rights_on <= next
            })
            .collect();
        if due.is_empty() {
            return Ok(false);
        }
        let book = due[0];
        let plan = book.plan();
        let fail = |detail: &str| crate::runtime::RuntimeError::ProtocolInvariant {
            detail: format!(
                "送转事件 {}，证券 {}，盘前日期 {next}：{detail}",
                plan.event_id, plan.stock.0
            ),
        };
        if due.len() != 1
            || self.state.setup.stocks.len() != 1
            || self.state.setup.dividend_tax_mode != Some(CashDividendTaxMode::Exempt)
            || !actions.dividend_tax_books.is_empty()
            || book.status() != &StockDistributionStatus::Registered
            || plan.ex_rights_on != next
        {
            return Err(fail(
                "仅支持单证券、Exempt、无个人税账且准时 Registered 的独立送转",
            ));
        }
        if actions.stock_distributions.iter().any(|row| {
            row.plan().event_id != plan.event_id
                && row.status() != &StockDistributionStatus::Credited
        }) || actions.dividends.iter().any(|row| {
            row.plan().stock == plan.stock
                && (row.plan().ex_dividend_on == next
                    || row.status() != &crate::company::cash_dividend::CashDividendStatus::Paid)
        }) || actions.rights_offerings.iter().any(|row| {
            row.plan().stock == plan.stock && row.status() != &RightsOfferingStatus::Settled
        }) || actions.share_splits.iter().any(|row| {
            row.plan().stock == plan.stock
                && row.status() != &crate::company::share_split::ShareSplitStatus::Settled
        }) || !actions.issuer_repurchases.is_empty()
            || actions
                .applied_ex_reference_groups
                .iter()
                .any(|group| group.stock == plan.stock && group.date == next)
        {
            return Err(fail("同证券重叠公司行为、Treasury 或重复除权不支持"));
        }
        let snapshot = book
            .registration()
            .ok_or_else(|| fail("缺少真实登记快照"))?;
        if !all_shares_circulating(snapshot)
            || snapshot
                .holdings()
                .iter()
                .any(|row| !matches!(row.holder, HolderId::Account(_)))
        {
            return Err(fail("当前仅支持全部流通且全部具名真实账户持有"));
        }
        let registry = actions
            .registries
            .iter()
            .find(|row| row.stock() == &plan.stock && row.issuer() == &plan.issuer)
            .ok_or_else(|| fail("缺少名册"))?;
        if registry.settled_on() != next.prev().map_err(|_| fail("盘前日期前驱无效"))?
            || registry.pending_opening().is_some()
        {
            return Err(fail("名册未完成前一自然日日结或已有 pending opening"));
        }
        Self::validate_opening_accounts(
            snapshot,
            baseline,
            !self.state.pending_player.is_empty(),
            fail,
        )?;
        Ok(true)
    }

    /// 盘前拆缩股只开放能从 Drain 后完整基线逐户勾稽的范围。
    pub(super) fn validate_split_opening_scope(
        &self,
        next: CivilDate,
        baseline: &super::civil_cash_distributions::CivilSettlementView,
    ) -> Result<bool, crate::runtime::RuntimeError> {
        use crate::company::share_split::ShareSplitStatus;
        let Some(actions) = self.state.companies.corporate_actions() else {
            return Ok(false);
        };
        let due: Vec<_> = actions
            .share_splits
            .iter()
            .filter(|book| {
                book.status() != &ShareSplitStatus::Settled && book.plan().ex_rights_on <= next
            })
            .collect();
        if due.is_empty() {
            return Ok(false);
        }
        let book = due[0];
        let plan = book.plan();
        let fail = |detail: &str| crate::runtime::RuntimeError::ProtocolInvariant {
            detail: format!(
                "拆缩股事件 {}，证券 {}，盘前日期 {next}：{detail}",
                plan.event_id, plan.stock.0
            ),
        };
        if due.len() != 1
            || self.state.setup.stocks.len() != 1
            || self.state.setup.dividend_tax_mode != Some(CashDividendTaxMode::Exempt)
            || !actions.dividend_tax_books.is_empty()
            || book.status() != &ShareSplitStatus::Registered
            || plan.ex_rights_on != next
        {
            return Err(fail(
                "仅支持单证券、Exempt、无个人税账且准时 Registered 的独立拆缩股",
            ));
        }
        if actions.share_splits.iter().any(|row| {
            row.plan().event_id != plan.event_id && row.status() != &ShareSplitStatus::Settled
        }) || actions.dividends.iter().any(|row| {
            row.plan().stock == plan.stock
                && (row.plan().ex_dividend_on == next
                    || row.status() != &crate::company::cash_dividend::CashDividendStatus::Paid)
        }) || actions.rights_offerings.iter().any(|row| {
            row.plan().stock == plan.stock && row.status() != &RightsOfferingStatus::Settled
        }) || actions.stock_distributions.iter().any(|row| {
            row.plan().stock == plan.stock
                && row.status()
                    != &crate::company::stock_distribution::StockDistributionStatus::Credited
        }) || !actions.issuer_repurchases.is_empty()
            || actions
                .applied_ex_reference_groups
                .iter()
                .any(|group| group.stock == plan.stock && group.date == next)
        {
            return Err(fail("同证券重叠公司行为、Treasury 或重复除权不支持"));
        }
        let snapshot = book
            .registration()
            .ok_or_else(|| fail("缺少真实登记快照"))?;
        if !all_shares_circulating(snapshot)
            || snapshot
                .holdings()
                .iter()
                .any(|row| !matches!(row.holder, HolderId::Account(_)))
        {
            return Err(fail("当前仅支持全部流通且全部具名真实账户持有"));
        }
        let registry = actions
            .registries
            .iter()
            .find(|row| row.stock() == &plan.stock && row.issuer() == &plan.issuer)
            .ok_or_else(|| fail("缺少名册"))?;
        if registry.settled_on() != next.prev().map_err(|_| fail("盘前日期前驱无效"))?
            || registry.pending_opening().is_some()
        {
            return Err(fail("名册未完成前一自然日日结或已有 pending opening"));
        }
        Self::validate_opening_accounts(
            snapshot,
            baseline,
            !self.state.pending_player.is_empty(),
            fail,
        )?;
        Ok(true)
    }

    fn validate_opening_accounts(
        snapshot: &RegistrationSnapshot,
        baseline: &super::civil_cash_distributions::CivilSettlementView,
        pending_player: bool,
        fail: impl Fn(&str) -> crate::runtime::RuntimeError,
    ) -> Result<(), crate::runtime::RuntimeError> {
        if baseline.active_plans || pending_player {
            return Err(fail("存在活动／待续行计划或待执行玩家委托"));
        }
        for (account, (_, _, raw)) in &baseline.accounts {
            let registered = snapshot
                .holdings()
                .iter()
                .find(|row| row.holder == HolderId::Account(*account))
                .map(|row| {
                    row.lots
                        .iter()
                        .try_fold(0u64, |sum, lot| sum.checked_add(lot.qty))
                })
                .unwrap_or(Some(0))
                .ok_or_else(|| fail("登记股数溢出"))?;
            if raw.shares.first().copied().map(u64::from) != Some(registered)
                || raw.pending_sell_qty.first() != Some(&0)
                || raw.foreign_active.first() != Some(&0)
                || raw.frozen_today.first() != Some(&0)
                || raw.buy_purse_held_cents != 0
            {
                return Err(fail(
                    "Drain／解冻后真实股份与登记不符，或仍有冻结／在途资源",
                ));
            }
        }
        if snapshot.holdings().iter().any(|row| {
            matches!(row.holder, HolderId::Account(account)
            if !baseline.accounts.contains_key(&account))
        }) {
            return Err(fail("登记股东不在真实 Owner 名册"));
        }
        Ok(())
    }

    pub(super) fn prepare_stock_ex_anchors(
        &mut self,
        next: CivilDate,
    ) -> Result<Vec<DeskExRightsRequest>, SessionError> {
        use crate::company::ex_reference_price::{
            stock_distribution_ex_rights_reference_price, StockDistributionExRightsFormula,
        };
        let Some(actions) = self.state.companies.corporate_actions() else {
            return Ok(Vec::new());
        };
        let mut groups = Vec::new();
        let mut requests = Vec::new();
        for book in actions
            .stock_distributions
            .iter()
            .filter(|book| book.credited_on() == Some(next))
        {
            let plan = book.plan();
            let fail = |detail: String| {
                SessionError::UnsupportedCapitalBoundary(format!(
                    "送转事件 {}，证券 {}，盘前日期 {next}：{detail}",
                    plan.event_id, plan.stock.0
                ))
            };
            let id = self
                .state
                .stocks
                .id(&plan.stock.0)
                .ok_or_else(|| fail("证券不在目录".into()))?;
            let market = self
                .state
                .markets
                .get(&id)
                .ok_or_else(|| fail("缺少真实收盘镜像".into()))?;
            let formula = match plan.exchange {
                crate::calendar::CalendarExchange::Sse => {
                    StockDistributionExRightsFormula::ShanghaiCirculatingShareChange {
                        ratio_micros: plan.shares_per_existing_share_micros,
                    }
                }
                crate::calendar::CalendarExchange::Szse => {
                    StockDistributionExRightsFormula::ShenzhenShareChange {
                        ratio_micros: plan.shares_per_existing_share_micros,
                    }
                }
            };
            let reference = stock_distribution_ex_rights_reference_price(
                self.state.civil_clock.calendar(),
                plan.exchange,
                formula,
                plan.registered_on,
                market.last_close(),
                Money::ZERO,
            )
            .map_err(|error| fail(error.to_string()))?;
            if reference.ex_date != next
                || actions
                    .applied_ex_reference_groups
                    .iter()
                    .any(|group| group.stock == plan.stock && group.date == next)
            {
                return Err(fail("除权日期不符或除权锚重复".into()));
            }
            requests.push(DeskExRightsRequest {
                stock: crate::runtime::StockIdx::try_from(id.0)
                    .map_err(|_| fail("证券坐标溢出".into()))?,
                expected_last_close: market.last_close().cents(),
                reference_price: reference.reference_price.cents(),
            });
            groups.push(AppliedExReferenceGroup {
                date: next,
                stock: plan.stock.clone(),
                cash_plan_ids: vec![],
                stock_event_ids: vec![plan.event_id.clone()],
                rights_event_ids: vec![],
                split_event_ids: vec![],
                pre_ex_close: market.last_close(),
                reference,
            });
        }
        if !groups.is_empty() {
            self.state
                .companies
                .corporate_actions_mut()
                .expect("候选来自同一公司容器")
                .applied_ex_reference_groups
                .extend(groups);
        }
        Ok(requests)
    }

    pub(super) fn prepare_split_ex_anchors(
        &mut self,
        next: CivilDate,
    ) -> Result<Vec<DeskExRightsRequest>, SessionError> {
        use crate::company::ex_reference_price::{
            share_split_ex_rights_reference_price, ShareSplitExRightsFormula,
        };
        let Some(actions) = self.state.companies.corporate_actions() else {
            return Ok(Vec::new());
        };
        let mut groups = Vec::new();
        let mut requests = Vec::new();
        for book in actions
            .share_splits
            .iter()
            .filter(|book| book.settled_on() == Some(next))
        {
            let plan = book.plan();
            let fail = |detail: String| {
                SessionError::UnsupportedCapitalBoundary(format!(
                    "拆缩股事件 {}，证券 {}，盘前日期 {next}：{detail}",
                    plan.event_id, plan.stock.0
                ))
            };
            let id = self
                .state
                .stocks
                .id(&plan.stock.0)
                .ok_or_else(|| fail("证券不在目录".into()))?;
            let market = self
                .state
                .markets
                .get(&id)
                .ok_or_else(|| fail("缺少真实收盘镜像".into()))?;
            let formula = match plan.exchange {
                crate::calendar::CalendarExchange::Sse => {
                    ShareSplitExRightsFormula::ShanghaiCirculatingShareChange {
                        numerator: plan.new_shares_per_old_share().0,
                        denominator: plan.new_shares_per_old_share().1,
                    }
                }
                crate::calendar::CalendarExchange::Szse => {
                    ShareSplitExRightsFormula::ShenzhenShareChange {
                        numerator: plan.new_shares_per_old_share().0,
                        denominator: plan.new_shares_per_old_share().1,
                    }
                }
            };
            let reference = share_split_ex_rights_reference_price(
                self.state.civil_clock.calendar(),
                plan.exchange,
                formula,
                plan.registered_on,
                market.last_close(),
                Money::ZERO,
            )
            .map_err(|error| fail(error.to_string()))?;
            if reference.ex_date != next
                || actions
                    .applied_ex_reference_groups
                    .iter()
                    .any(|group| group.stock == plan.stock && group.date == next)
            {
                return Err(fail("除权日期不符或除权锚重复".into()));
            }
            requests.push(DeskExRightsRequest {
                stock: crate::runtime::StockIdx::try_from(id.0)
                    .map_err(|_| fail("证券坐标溢出".into()))?,
                expected_last_close: market.last_close().cents(),
                reference_price: reference.reference_price.cents(),
            });
            groups.push(AppliedExReferenceGroup {
                date: next,
                stock: plan.stock.clone(),
                cash_plan_ids: vec![],
                stock_event_ids: vec![],
                rights_event_ids: vec![],
                split_event_ids: vec![plan.event_id.clone()],
                pre_ex_close: market.last_close(),
                reference,
            });
        }
        if !groups.is_empty() {
            self.state
                .companies
                .corporate_actions_mut()
                .expect("候选来自同一公司容器")
                .applied_ex_reference_groups
                .extend(groups);
        }
        Ok(requests)
    }

    pub(super) fn prepare_rights_ex_anchors(
        &mut self,
        next_date: CivilDate,
    ) -> Result<Vec<DeskExRightsRequest>, SessionError> {
        if !matches!(
            self.state.setup.dividend_tax_mode,
            Some(CashDividendTaxMode::Exempt | CashDividendTaxMode::FlatWithholding)
        ) {
            return Ok(Vec::new());
        }
        let Some(actions) = self.state.companies.corporate_actions() else {
            return Ok(Vec::new());
        };
        let due: Vec<_> = actions
            .rights_offerings
            .iter()
            .filter(|book| {
                book.status() == &RightsOfferingStatus::Closed
                    && book.plan().ex_rights_on == next_date
            })
            .cloned()
            .collect();
        let mut requests = Vec::new();
        let mut groups = Vec::new();
        let mut seen = BTreeSet::new();
        for book in due {
            let plan = book.plan();
            let fail = |detail: String| {
                SessionError::UnsupportedCapitalBoundary(format!(
                    "配股事件 {}：{detail}",
                    plan.event_id
                ))
            };
            if book
                .determine_failure()
                .map_err(|error| fail(error.to_string()))?
            {
                continue;
            }
            if !actions.dividend_tax_books.is_empty()
                || !matches!(
                    &plan.mode,
                    RightsOfferingMode::RightsToAllShareholders { .. }
                )
            {
                return Err(fail("除权仅支持无个人税账的面向全体股东配股".into()));
            }
            if !seen.insert(plan.stock.clone())
                || actions.dividends.iter().any(|row| {
                    row.plan().stock == plan.stock && row.plan().ex_dividend_on == next_date
                })
                || actions.stock_distributions.iter().any(|row| {
                    row.plan().stock == plan.stock && row.plan().ex_rights_on == next_date
                })
                || actions.share_splits.iter().any(|row| {
                    row.plan().stock == plan.stock && row.plan().ex_rights_on == next_date
                })
                || actions
                    .applied_ex_reference_groups
                    .iter()
                    .any(|row| row.stock == plan.stock && row.date == next_date)
            {
                return Err(fail("同证券同日组合或重复除权尚不支持".into()));
            }
            let snapshot = book
                .registration()
                .ok_or_else(|| fail("缺少登记快照".into()))?;
            if !all_shares_circulating(snapshot) {
                return Err(fail("当前除权要求全部股份流通".into()));
            }
            let paid = book
                .subscriptions()
                .iter()
                .try_fold(0u64, |sum, row| sum.checked_add(row.paid_shares))
                .ok_or_else(|| fail("实缴股份合计溢出".into()))?;
            let formula = match plan.exchange {
                crate::calendar::CalendarExchange::Sse => {
                    RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChangeRational {
                        issued_shares: paid,
                        base_shares: snapshot.issued_shares(),
                    }
                }
                crate::calendar::CalendarExchange::Szse => {
                    RightsOfferingExRightsFormula::ShenzhenRightsShareChangeRational {
                        issued_shares: paid,
                        base_shares: snapshot.issued_shares(),
                    }
                }
            };
            let id = self
                .state
                .stocks
                .id(&plan.stock.0)
                .ok_or_else(|| fail("证券不在目录".into()))?;
            let market = self
                .state
                .markets
                .get(&id)
                .ok_or_else(|| fail("缺少收盘镜像".into()))?;
            let reference = rights_offering_ex_rights_reference_price(
                self.state.civil_clock.calendar(),
                plan.exchange,
                formula,
                plan.payment_deadline_on,
                market.last_close(),
                Money::ZERO,
                plan.price_per_share,
            )
            .map_err(|error| fail(error.to_string()))?;
            if reference.ex_date != next_date {
                return Err(fail("除权日期与方案不一致".into()));
            }
            let stock = crate::runtime::StockIdx::try_from(id.0)
                .map_err(|_| fail("证券坐标溢出".into()))?;
            requests.push(DeskExRightsRequest {
                stock,
                expected_last_close: market.last_close().cents(),
                reference_price: reference.reference_price.cents(),
            });
            groups.push(AppliedExReferenceGroup {
                date: next_date,
                stock: plan.stock.clone(),
                cash_plan_ids: Vec::new(),
                stock_event_ids: Vec::new(),
                rights_event_ids: vec![plan.event_id.clone()],
                split_event_ids: Vec::new(),
                pre_ex_close: market.last_close(),
                reference,
            });
        }
        self.state
            .companies
            .corporate_actions_mut()
            .ok_or_else(|| SessionError::UnsupportedCapitalBoundary("公司行为容器消失".into()))?
            .applied_ex_reference_groups
            .extend(groups);
        Ok(requests)
    }
}
