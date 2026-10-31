//! 读取日终存档时核验配股／送转除权锚、冻结来源和 opening 消费。
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
        let reference = if !group.stock_event_ids.is_empty() {
            validate_stock_group(save, actions, group, &calendar)?
        } else if !group.split_event_ids.is_empty() {
            validate_split_group(save, actions, group, &calendar)?
        } else if !group.rights_event_ids.is_empty() {
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
                group.pre_ex_close,
                Money::ZERO,
                plan.price_per_share,
            )
            .map_err(|e| invalid(e.to_string()))?;
            reference
        } else {
            continue;
        };
        if reference != group.reference {
            return Err(invalid("冻结收盘与除权锚不一致".into()));
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
    for book in &actions.stock_distributions {
        if book.credited_on().is_some() || book.plan().ex_rights_on <= save.civil_clock.current_date
        {
            let matches = actions
                .applied_ex_reference_groups
                .iter()
                .filter(|group| {
                    group.stock == book.plan().stock
                        && group.date == book.plan().ex_rights_on
                        && group.stock_event_ids == [book.plan().event_id.clone()]
                })
                .count();
            if matches != 1 {
                return Err(invalid(format!(
                    "送转事件 {} 缺少唯一真实除权锚",
                    book.plan().event_id
                )));
            }
        }
    }
    for book in &actions.share_splits {
        if (book.settled_on().is_some()
            || book.plan().ex_rights_on <= save.civil_clock.current_date)
            && actions
                .applied_ex_reference_groups
                .iter()
                .filter(|group| {
                    group.stock == book.plan().stock
                        && group.date == book.plan().ex_rights_on
                        && group.split_event_ids == [book.plan().event_id.clone()]
                })
                .count()
                != 1
        {
            return Err(invalid(format!(
                "拆缩股事件 {} 缺少唯一真实除权锚",
                book.plan().event_id
            )));
        }
    }
    for registry in &actions.registries {
        if let Some(opening) = registry.pending_opening() {
            if !save.plans.active_plan_ids().is_empty()
                || !save.pending_player.is_empty()
                || opening.request.day != save.civil_clock.current_date
                || !(actions.stock_distributions.iter().any(|book| {
                    book.plan().stock == *registry.stock()
                        && book.credited_on() == Some(opening.request.day)
                        && opening.request.event_id
                            == format!("stock-distribution:{}", book.plan().event_id)
                }) || actions.share_splits.iter().any(|book| {
                    book.plan().stock == *registry.stock()
                        && book.settled_on() == Some(opening.request.day)
                        && opening.request.event_id
                            == format!("share-transform:{}", book.plan().event_id)
                }))
            {
                return Err(invalid("pending opening 缺少当前盘前送转事件".into()));
            }
            let id = catalog
                .id(&registry.stock().0)
                .ok_or_else(|| invalid("盘前证券不在目录".into()))?;
            for (owner, account) in &save.snapshot.accounts {
                let expected = registry
                    .holdings()
                    .iter()
                    .find(|row| {
                        row.holder == crate::company::share_registry::HolderId::Account(*owner)
                    })
                    .map(|row| {
                        row.lots
                            .iter()
                            .try_fold(0u64, |sum, lot| sum.checked_add(lot.qty))
                    })
                    .unwrap_or(Some(0))
                    .ok_or_else(|| invalid("盘前股份合计溢出".into()))?;
                let actual = account
                    .positions
                    .get(&id)
                    .map(|position| u64::from(position.qty))
                    .unwrap_or(0);
                if actual != expected
                    || account
                        .positions
                        .get(&id)
                        .is_some_and(|position| position.t1_locked != 0)
                {
                    return Err(invalid(format!("盘前 REG 与账户 {owner:?} 的股份不一致")));
                }
            }
            if registry
                .holdings()
                .iter()
                .any(|holding| match holding.holder {
                    crate::company::share_registry::HolderId::Account(owner) => {
                        !save.snapshot.accounts.contains_key(&owner)
                    }
                    _ => true,
                })
            {
                return Err(invalid("盘前 REG 股东缺少真实账户".into()));
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

fn validate_stock_group(
    save: &SaveSlot,
    actions: &crate::session::SessionCorporateActions,
    group: &crate::session::corporate_actions::AppliedExReferenceGroup,
    calendar: &TradingCalendar,
) -> Result<crate::company::ex_reference_price::ExReferencePrice, ShareRegistryError> {
    use crate::company::ex_reference_price::{
        stock_distribution_ex_rights_reference_price, StockDistributionExRightsFormula,
    };
    let invalid = |detail: &str| ShareRegistryError::InvalidFact {
        detail: format!("送转除权 {} {}：{detail}", group.stock.0, group.date),
    };
    if group.stock_event_ids.len() != 1
        || !group.rights_event_ids.is_empty()
        || !group.cash_plan_ids.is_empty()
        || !group.split_event_ids.is_empty()
        || save.setup.stocks.len() != 1
        || save.setup.issuer_repurchase_enabled
        || save.setup.dividend_tax_mode != Some(CashDividendTaxMode::Exempt)
        || !actions.dividend_tax_books.is_empty()
    {
        return Err(invalid("仅支持单证券 Exempt 独立送转"));
    }
    let book = actions
        .stock_distributions
        .iter()
        .find(|book| book.plan().event_id == group.stock_event_ids[0])
        .ok_or_else(|| invalid("除权锚缺少送转事件"))?;
    let plan = book.plan();
    if plan.stock != group.stock
        || plan.ex_rights_on != group.date
        || book.credited_on() != Some(group.date)
    {
        return Err(invalid("除权锚与入账事件不一致"));
    }
    let snapshot = book.registration().ok_or_else(|| invalid("送转缺登记"))?;
    if !crate::session::civil_rights_ex::all_shares_circulating(snapshot)
        || snapshot.holdings().iter().any(|holding| {
            !matches!(
                holding.holder,
                crate::company::share_registry::HolderId::Account(_)
            )
        })
    {
        return Err(invalid("送转要求全部流通且全部为真实账户股份"));
    }
    let registry = actions
        .registries
        .iter()
        .find(|registry| registry.stock() == &plan.stock && registry.issuer() == &plan.issuer)
        .ok_or_else(|| invalid("送转缺少名册"))?;
    let movement = format!("stock-distribution:{}", plan.event_id);
    let bound = if registry.settled_on() < group.date {
        registry.pending_opening().map(|opening| &opening.reference)
    } else {
        let mut refs =
            registry
                .receipts()
                .iter()
                .filter_map(|receipt| match &receipt.request.scope {
                    crate::company::share_registry::MovementScope::SettlementDayMarket {
                        binding,
                    } if receipt.request.day == group.date => binding
                        .opening
                        .iter()
                        .find(|opening| opening.movement_event_id == movement),
                    _ => None,
                });
        let first = refs.next();
        if refs.next().is_some() {
            return Err(invalid("送转 opening 被重复消费"));
        }
        first
    }
    .ok_or_else(|| invalid("送转缺少 pending 或已消费的 opening"))?;
    if bound.movement_event_id != movement
        || bound.registration_event_id != plan.event_id
        || bound.from_unit != bound.to_unit
    {
        return Err(invalid("送转 opening 来源或单位不一致"));
    }
    let formula = match plan.exchange {
        CalendarExchange::Sse => StockDistributionExRightsFormula::ShanghaiCirculatingShareChange {
            ratio_micros: plan.shares_per_existing_share_micros,
        },
        CalendarExchange::Szse => StockDistributionExRightsFormula::ShenzhenShareChange {
            ratio_micros: plan.shares_per_existing_share_micros,
        },
    };
    stock_distribution_ex_rights_reference_price(
        calendar,
        plan.exchange,
        formula,
        plan.registered_on,
        group.pre_ex_close,
        Money::ZERO,
    )
    .map_err(|error| ShareRegistryError::InvalidFact {
        detail: error.to_string(),
    })
}

fn validate_split_group(
    save: &SaveSlot,
    actions: &crate::session::SessionCorporateActions,
    group: &crate::session::corporate_actions::AppliedExReferenceGroup,
    calendar: &TradingCalendar,
) -> Result<crate::company::ex_reference_price::ExReferencePrice, ShareRegistryError> {
    use crate::company::ex_reference_price::{
        share_split_ex_rights_reference_price, ShareSplitExRightsFormula,
    };
    let invalid = |detail: &str| ShareRegistryError::InvalidFact {
        detail: format!("拆缩股除权 {} {}：{detail}", group.stock.0, group.date),
    };
    if group.split_event_ids.len() != 1
        || !group.rights_event_ids.is_empty()
        || !group.cash_plan_ids.is_empty()
        || !group.stock_event_ids.is_empty()
        || save.setup.stocks.len() != 1
        || save.setup.issuer_repurchase_enabled
        || save.setup.dividend_tax_mode != Some(CashDividendTaxMode::Exempt)
        || !actions.dividend_tax_books.is_empty()
    {
        return Err(invalid("仅支持单证券 Exempt 独立拆缩股"));
    }
    let book = actions
        .share_splits
        .iter()
        .find(|book| book.plan().event_id == group.split_event_ids[0])
        .ok_or_else(|| invalid("除权锚缺少拆缩股事件"))?;
    let plan = book.plan();
    if plan.stock != group.stock
        || plan.ex_rights_on != group.date
        || book.settled_on() != Some(group.date)
    {
        return Err(invalid("除权锚与入账事件不一致"));
    }
    let snapshot = book.registration().ok_or_else(|| invalid("拆缩股缺登记"))?;
    if !crate::session::civil_rights_ex::all_shares_circulating(snapshot)
        || snapshot.holdings().iter().any(|holding| {
            !matches!(
                holding.holder,
                crate::company::share_registry::HolderId::Account(_)
            )
        })
    {
        return Err(invalid("拆缩股要求全部流通且全部为真实账户股份"));
    }
    let registry = actions
        .registries
        .iter()
        .find(|registry| registry.stock() == &plan.stock && registry.issuer() == &plan.issuer)
        .ok_or_else(|| invalid("拆缩股缺少名册"))?;
    let movement = format!("share-transform:{}", plan.event_id);
    let bound = if registry.settled_on() < group.date {
        registry.pending_opening().map(|opening| &opening.reference)
    } else {
        let mut refs =
            registry
                .receipts()
                .iter()
                .filter_map(|receipt| match &receipt.request.scope {
                    crate::company::share_registry::MovementScope::SettlementDayMarket {
                        binding,
                    } if receipt.request.day == group.date => binding
                        .opening
                        .iter()
                        .find(|opening| opening.movement_event_id == movement),
                    _ => None,
                });
        let first = refs.next();
        if refs.next().is_some() {
            return Err(invalid("拆缩股 opening 被重复消费"));
        }
        first
    }
    .ok_or_else(|| invalid("拆缩股缺少 pending 或已消费的 opening"))?;
    if bound.movement_event_id != movement
        || bound.registration_event_id != plan.event_id
        || bound.from_unit == bound.to_unit
        || bound.to_unit
            != (crate::company::share_registry::settlement_day::ShareUnitIdentity::ReDenominated {
                movement_event_id: movement.clone(),
            })
    {
        return Err(invalid("拆缩股 opening 来源或单位不一致"));
    }
    let formula = match plan.exchange {
        CalendarExchange::Sse => ShareSplitExRightsFormula::ShanghaiCirculatingShareChange {
            numerator: plan.new_shares_per_old_share().0,
            denominator: plan.new_shares_per_old_share().1,
        },
        CalendarExchange::Szse => ShareSplitExRightsFormula::ShenzhenShareChange {
            numerator: plan.new_shares_per_old_share().0,
            denominator: plan.new_shares_per_old_share().1,
        },
    };
    share_split_ex_rights_reference_price(
        calendar,
        plan.exchange,
        formula,
        plan.registered_on,
        group.pre_ex_close,
        Money::ZERO,
    )
    .map_err(|error| ShareRegistryError::InvalidFact {
        detail: error.to_string(),
    })
}
