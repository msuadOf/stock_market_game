use super::{P3OpenOrderLimits, P3StockValidation, P3ValidationContext, StepFatal};
use crate::{AccountId, GameSession, SecurityCategory, StockCode};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct P3StockContextFacts {
    pub(super) category: SecurityCategory,
    pub(super) market_buy_protective_price: crate::Money,
    pub(super) market_sell_protective_price: crate::Money,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct P3ContextFacts {
    pub(super) stocks: BTreeMap<StockCode, P3StockContextFacts>,
    pub(super) global_open_orders: usize,
    pub(super) account_open_orders: BTreeMap<AccountId, usize>,
}

/// Captures the real post-P0 facts consumed by P3 without deriving board semantics
/// from a stock-code prefix or inventing protective prices for market orders.
pub(super) fn build_p3_validation_context(
    session: &GameSession,
) -> Result<P3ValidationContext, StepFatal> {
    let facts = collect_p3_context_facts(session)?;
    let blocked = if session.has_pending_plan_event_capacity(2) {
        Vec::new()
    } else {
        session
            .parent_orders
            .iter()
            .flat_map(|(account, parents)| {
                parents.iter().filter_map(|(code, parent)| {
                    parent
                        .linked_plan_id
                        .is_some()
                        .then_some((*account, code.clone()))
                })
            })
            .collect()
    };
    P3ValidationContext::new(
        facts.stocks.into_iter().map(|(code, stock)| {
            (
                code,
                P3StockValidation::new(
                    stock.category,
                    stock.market_buy_protective_price,
                    stock.market_sell_protective_price,
                ),
            )
        }),
        facts.global_open_orders,
        facts.account_open_orders,
        P3OpenOrderLimits::PRODUCTION,
    )
    .map(|context| context.with_pending_plan_event_blocks(blocked))
}

pub(super) fn collect_p3_context_facts(session: &GameSession) -> Result<P3ContextFacts, StepFatal> {
    let mut stocks = BTreeMap::new();
    for (code, market) in &session.markets {
        let spec = session
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == *code)
            .ok_or_else(|| invariant("configured market has no stock specification"))?;
        let market_buy_protective_price = market
            .up_stop()
            .map_err(|error| invariant(&error.to_string()))?;
        let market_sell_protective_price = market
            .down_stop()
            .map_err(|error| invariant(&error.to_string()))?;
        stocks.insert(
            code.clone(),
            P3StockContextFacts {
                category: spec.category,
                market_buy_protective_price,
                market_sell_protective_price,
            },
        );
    }
    if stocks.len() != session.setup.stocks.len() {
        return Err(invariant(
            "stock specifications and configured markets are not one-to-one",
        ));
    }

    let mut account_open_orders = session
        .accounts
        .keys()
        .copied()
        .map(|account| (account, 0_usize))
        .collect::<BTreeMap<_, _>>();
    let mut global_open_orders = 0_usize;
    for market in session.markets.values() {
        for order in market.resting_orders() {
            global_open_orders = checked_increment(global_open_orders, "global open-order count")?;
            let count = account_open_orders
                .get_mut(&order.owner)
                .ok_or_else(|| invariant("continuous order owner account is missing"))?;
            *count = checked_increment(*count, "account open-order count")?;
        }
    }
    let mut actual_auction_counts = BTreeMap::<AccountId, usize>::new();
    for orders in session.auction_orders.values() {
        for order in orders {
            global_open_orders = checked_increment(global_open_orders, "global open-order count")?;
            let count = account_open_orders
                .get_mut(&order.owner)
                .ok_or_else(|| invariant("auction order owner account is missing"))?;
            *count = checked_increment(*count, "account open-order count")?;
            let auction_count = actual_auction_counts.entry(order.owner).or_default();
            *auction_count = checked_increment(*auction_count, "auction order count")?;
        }
    }
    if actual_auction_counts != session.auction_order_counts {
        return Err(invariant(
            "auction order count cache disagrees with authoritative auction orders",
        ));
    }

    Ok(P3ContextFacts {
        stocks,
        global_open_orders,
        account_open_orders,
    })
}

pub(super) fn checked_increment(value: usize, label: &str) -> Result<usize, StepFatal> {
    value
        .checked_add(1)
        .ok_or_else(|| invariant(&format!("{label} overflow")))
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p3_context".to_owned(),
    }
}
