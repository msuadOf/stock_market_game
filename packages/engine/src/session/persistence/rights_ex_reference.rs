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

pub(in crate::session) fn validate(
    save: &SaveSlot,
    catalog: &StockCatalog,
) -> Result<(), ShareRegistryError> {
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
        if !crate::session::civil_rights_ex::all_shares_circulating(snapshot) {
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
        let formula = match plan.exchange {
            CalendarExchange::Sse => {
                RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChange {
                    ratio_micros: ratio,
                }
            }
            CalendarExchange::Szse => RightsOfferingExRightsFormula::ShenzhenRightsShareChange {
                ratio_micros: ratio,
            },
        };
        let reference = rights_offering_ex_rights_reference_price(
            &calendar,
            plan.exchange,
            formula,
            plan.payment_deadline_on,
            group.pre_ex_close,
            Money::ZERO,
            plan.price_per_share,
        )
        .map_err(|e| invalid(e.to_string()))?;
        if reference != group.reference {
            return Err(invalid("冻结收盘与配股除权锚不一致".into()));
        }
        if group.date == save.civil_clock.current_date {
            let id = catalog
                .id(&group.stock.0)
                .ok_or_else(|| invalid("除权证券不在目录".into()))?;
            let market = save
                .snapshot
                .markets
                .get(&id)
                .ok_or_else(|| invalid("除权存档缺市场".into()))?;
            let clock = super::super::CivilClock::from_parts_for_exchanges(
                save.setup.start_date,
                &save.civil_clock,
                save.setup.stocks.iter().map(|stock| {
                    super::super::civil_clock::session_calendar_exchange(stock.exchange)
                }),
            )
            .map_err(|e| invalid(e.to_string()))?;
            let source =
                super::super::GameSession::calendar_source_for(&save.setup, catalog, &clock)
                    .map_err(|e| invalid(e.to_string()))?;
            let day = source.day(group.date).map_err(|e| invalid(e.to_string()))?;
            let layout = super::super::exchange_calendar_model::SessionTickLayout {
                ticks_per_day: save.setup.ticks_per_day,
                auction_ticks: save.setup.auction_ticks,
                closing_auction_ticks: save.setup.closing_auction_ticks,
            };
            let progress = super::super::market_calendar_live::calendar_clock_input(
                &day,
                layout,
                save.snapshot.tick,
                save.snapshot.tick / save.setup.ticks_per_day,
            )
            .map_err(|e| invalid(e.to_string()))?
            .progress;
            let expected = if matches!(
                progress,
                super::super::exchange_calendar_model::SharedDayProgress::TradingCompleted
            ) {
                let candle = save
                    .snapshot
                    .daily_candles
                    .get(&id)
                    .and_then(|rows| rows.last())
                    .ok_or_else(|| invalid("除权日收盘缺日 K".into()))?;
                if candle.volume > 0 {
                    market.last_price
                } else {
                    reference.reference_price
                }
            } else {
                reference.reference_price
            };
            if market.last_close != expected {
                return Err(invalid("存档市场与配股除权锚不一致".into()));
            }
        }
    }
    if save.setup.dividend_tax_mode == Some(CashDividendTaxMode::Exempt) {
        for book in &actions.rights_offerings {
            if matches!(
                book.status(),
                RightsOfferingStatus::Closed | RightsOfferingStatus::Settled
            ) && book.plan().ex_rights_on <= save.civil_clock.current_date
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
