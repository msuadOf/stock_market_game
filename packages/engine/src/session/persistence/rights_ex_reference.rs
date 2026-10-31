//! 读取日终存档时核验配股除权锚与真实方案、登记及实缴事实。
use crate::calendar::{CalendarExchange, CalendarPolicy, TradingCalendar};
use crate::company::cash_dividend_tax::CashDividendTaxMode;
use crate::company::ex_reference_price::{
    rights_offering_ex_rights_reference_price, RightsOfferingExRightsFormula,
};
use crate::company::rights_offering::{RightsOfferingMode, RightsOfferingStatus};
use crate::company::share_registry::ShareRegistryError;
use crate::session::SaveSlot;
use crate::stock::StockCatalog;
use crate::Money;
use std::collections::BTreeSet;

pub(super) fn validate(save: &SaveSlot, catalog: &StockCatalog) -> Result<(), ShareRegistryError> {
    let Some(actions) = save.company_state.actions() else {
        return Ok(());
    };
    let invalid = |detail: String| ShareRegistryError::InvalidFact { detail };
    let mut seen = BTreeSet::new();
    let calendar = TradingCalendar::from_policy(
        CalendarPolicy::from_parts(save.civil_clock.policy.clone())
            .map_err(|e| invalid(e.to_string()))?,
    )
    .map_err(|e| invalid(e.to_string()))?;
    for group in &actions.applied_ex_reference_groups {
        if !seen.insert((group.stock.clone(), group.date))
            || group.reference.ex_date != group.date
            || group.date > save.civil_clock.current_date
        {
            return Err(invalid("除权锚重复、日期不符或超前".into()));
        }
        if group.rights_event_ids.is_empty() {
            continue;
        }
        if group.rights_event_ids.len() != 1
            || !group.cash_plan_ids.is_empty()
            || !group.stock_event_ids.is_empty()
            || !group.split_event_ids.is_empty()
            || save.setup.dividend_tax_mode != Some(CashDividendTaxMode::Exempt)
            || !actions.dividend_tax_books.is_empty()
        {
            return Err(invalid("配股除权锚包含不支持的组合或税制".into()));
        }
        let event = &group.rights_event_ids[0];
        let book = actions
            .rights_offerings
            .iter()
            .find(|book| book.plan().event_id == *event)
            .ok_or_else(|| invalid(format!("除权锚缺配股事件 {event}")))?;
        let plan = book.plan();
        if plan.stock != group.stock
            || plan.ex_rights_on != group.date
            || !matches!(
                &plan.mode,
                RightsOfferingMode::RightsToAllShareholders { .. }
            )
            || !matches!(
                book.status(),
                RightsOfferingStatus::Closed | RightsOfferingStatus::Settled
            )
            || book
                .determine_failure()
                .map_err(|e| invalid(e.to_string()))?
        {
            return Err(invalid(format!("除权锚与成功配股事件 {event} 不符")));
        }
        let snapshot = book
            .registration()
            .ok_or_else(|| invalid(format!("除权事件 {event} 缺登记")))?;
        let spec = save
            .setup
            .stocks
            .iter()
            .find(|spec| spec.code == group.stock)
            .ok_or_else(|| invalid("除权证券缺规格".into()))?;
        if u64::from(spec.float_shares) != snapshot.issued_shares() {
            return Err(invalid("配股除权要求全部股份流通".into()));
        }
        let paid = book
            .subscriptions()
            .iter()
            .try_fold(0u64, |sum, row| sum.checked_add(row.paid_shares))
            .ok_or_else(|| invalid("配股股份溢出".into()))?;
        let ratio =
            crate::session::civil_rights_ex::exact_ratio_micros(paid, snapshot.issued_shares())
                .map_err(|e| invalid(e.into()))?;
        if group.date == save.civil_clock.current_date {
            let id = catalog
                .id(&group.stock.0)
                .ok_or_else(|| invalid("除权证券不在目录".into()))?;
            let market = save
                .snapshot
                .markets
                .get(&id)
                .ok_or_else(|| invalid("除权存档缺市场".into()))?;
            let formula = match plan.exchange {
                CalendarExchange::Sse => {
                    RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChange {
                        ratio_micros: ratio,
                    }
                }
                CalendarExchange::Szse => {
                    RightsOfferingExRightsFormula::ShenzhenRightsShareChange {
                        ratio_micros: ratio,
                    }
                }
            };
            let reference = rights_offering_ex_rights_reference_price(
                &calendar,
                plan.exchange,
                formula,
                plan.payment_deadline_on,
                market.last_price,
                Money::ZERO,
                plan.price_per_share,
            )
            .map_err(|e| invalid(e.to_string()))?;
            if reference != group.reference || market.last_close != reference.reference_price {
                return Err(invalid("存档市场与配股除权锚不一致".into()));
            }
        }
    }
    if save.setup.dividend_tax_mode == Some(CashDividendTaxMode::Exempt) {
        for book in &actions.rights_offerings {
            if book.status() == &RightsOfferingStatus::Closed
                && book.plan().ex_rights_on <= save.civil_clock.current_date
                && !book
                    .determine_failure()
                    .map_err(|error| invalid(error.to_string()))?
                && !actions.applied_ex_reference_groups.iter().any(|group| {
                    group.stock == book.plan().stock
                        && group.date == book.plan().ex_rights_on
                        && group.rights_event_ids == [book.plan().event_id.clone()]
                })
            {
                return Err(invalid(format!(
                    "成功配股事件 {} 缺少真实除权锚",
                    book.plan().event_id
                )));
            }
        }
    }
    Ok(())
}
