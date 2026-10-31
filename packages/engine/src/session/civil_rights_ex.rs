//! 配股除权由当日日终真实公司候选与交易部收盘共同准备。
use super::*;
use crate::company::cash_dividend_tax::CashDividendTaxMode;
use crate::company::ex_reference_price::{
    rights_offering_ex_rights_reference_price, RightsOfferingExRightsFormula,
};
use crate::company::rights_offering::{RightsOfferingMode, RightsOfferingStatus};
use crate::company::share_registry::{HolderId, ShareRestriction};
use crate::runtime::protocol::DeskExRightsRequest;
use crate::session::corporate_actions::AppliedExReferenceGroup;

pub(super) fn exact_ratio_micros(issued: u64, base: u64) -> Result<u64, &'static str> {
    if base == 0 || issued == 0 {
        return Err("配股除权股数必须为正");
    }
    let scaled = u128::from(issued) * 1_000_000;
    if scaled % u128::from(base) != 0 {
        return Err("实际配股比例不能精确表示为百万分之一");
    }
    u64::try_from(scaled / u128::from(base)).map_err(|_| "配股除权比例溢出")
}

impl GameSession {
    pub(super) fn prepare_rights_ex_anchors(
        &mut self,
        next_date: CivilDate,
    ) -> Result<Vec<DeskExRightsRequest>, SessionError> {
        if self.state.setup.dividend_tax_mode != Some(CashDividendTaxMode::Exempt) {
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
            if snapshot.holdings().iter().any(|holding| {
                holding.holder == HolderId::IssuerTreasury
                    || holding
                        .lots
                        .iter()
                        .any(|lot| lot.restriction != ShareRestriction::Unrestricted)
            }) {
                return Err(fail("当前除权不支持库存股或限售股份".into()));
            }
            let spec = self
                .state
                .setup
                .stocks
                .iter()
                .find(|spec| spec.code == plan.stock)
                .ok_or_else(|| fail("证券缺少开局规格".into()))?;
            if u64::from(spec.float_shares) != snapshot.issued_shares() {
                return Err(fail("当前除权要求全部股份流通".into()));
            }
            let paid = book
                .subscriptions()
                .iter()
                .try_fold(0u64, |sum, row| sum.checked_add(row.paid_shares))
                .ok_or_else(|| fail("实缴股份合计溢出".into()))?;
            let ratio_micros = exact_ratio_micros(paid, snapshot.issued_shares())
                .map_err(|detail| fail(detail.into()))?;
            let formula = match plan.exchange {
                crate::calendar::CalendarExchange::Sse => {
                    RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChange { ratio_micros }
                }
                crate::calendar::CalendarExchange::Szse => {
                    RightsOfferingExRightsFormula::ShenzhenRightsShareChange { ratio_micros }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rights_ratio_does_not_silently_round() {
        assert_eq!(exact_ratio_micros(20, 200).unwrap(), 100_000);
        assert!(exact_ratio_micros(1, 3).is_err());
        assert!(exact_ratio_micros(0, 200).is_err());
    }
}
