use super::{P3OpenOrderLimits, P3StockValidation, P3ValidationContext, StepFatal};
use crate::{AccountId, GameSession, SecurityCategory, StockCode};
use rayon::prelude::*;
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
    let linked_parents = session
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
        .collect::<Vec<_>>();
    let pending_plan_event_slots = crate::session::MAX_SAVED_PLAN_EVENTS
        .checked_sub(session.pending_plan_events.len())
        .ok_or_else(|| invariant("pending plan events exceed their runtime capacity"))?;
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
    .map(|context| context.with_pending_plan_event_budget(linked_parents, pending_plan_event_slots))
}

pub(super) fn collect_p3_context_facts(session: &GameSession) -> Result<P3ContextFacts, StepFatal> {
    if session.markets.len() != session.setup.stocks.len() {
        return Err(invariant(
            "stock specifications and configured markets are not one-to-one",
        ));
    }
    let mut specifications = BTreeMap::new();
    for stock in &session.setup.stocks {
        if specifications.insert(&stock.code, stock.category).is_some() {
            return Err(invariant("duplicate stock specification in P3 context"));
        }
    }
    let (market_shards, auction_shards) = rayon::join(
        || {
            session
                .markets
                .par_iter()
                .map(|(code, market)| {
                    let category = *specifications
                        .get(code)
                        .ok_or_else(|| invariant("configured market has no stock specification"))?;
                    let stock = P3StockContextFacts {
                        category,
                        market_buy_protective_price: market
                            .up_stop()
                            .map_err(|error| invariant(&error.to_string()))?,
                        market_sell_protective_price: market
                            .down_stop()
                            .map_err(|error| invariant(&error.to_string()))?,
                    };
                    let mut owner_counts = Vec::new();
                    let mut counted = 0_usize;
                    for (owner, count) in market.resting_order_counts_by_owner() {
                        if !session.accounts.contains_key(&owner) {
                            return Err(invariant("continuous order owner account is missing"));
                        }
                        if count == 0 {
                            return Err(invariant("continuous order owner count is zero"));
                        }
                        counted = checked_add(counted, count, "global open-order count")?;
                        owner_counts.push((owner, count));
                    }
                    if counted != market.resting_order_count() {
                        return Err(invariant(
                            "continuous order owner count total disagrees with book",
                        ));
                    }
                    Ok((code.clone(), stock, counted, owner_counts))
                })
                .collect::<Result<Vec<_>, StepFatal>>()
        },
        || {
            session
                .auction_orders
                .par_iter()
                .map(|(_, orders)| {
                    let mut owner_counts = BTreeMap::new();
                    for order in orders {
                        if !session.accounts.contains_key(&order.owner) {
                            return Err(invariant("auction order owner account is missing"));
                        }
                        let count = owner_counts.entry(order.owner).or_insert(0_usize);
                        *count = checked_increment(*count, "auction order count")?;
                    }
                    Ok((orders.len(), owner_counts))
                })
                .collect::<Result<Vec<_>, StepFatal>>()
        },
    );
    let mut stocks = BTreeMap::new();
    let mut account_open_orders = BTreeMap::<AccountId, usize>::new();
    let mut global_open_orders = 0_usize;
    for (code, stock, count, owners) in market_shards? {
        global_open_orders = checked_add(global_open_orders, count, "global open-order count")?;
        for (owner, count) in owners {
            let account_count = account_open_orders.entry(owner).or_default();
            *account_count = checked_add(*account_count, count, "account open-order count")?;
        }
        stocks.insert(code, stock);
    }
    let mut actual_auction_counts = BTreeMap::<AccountId, usize>::new();
    for (total, owners) in auction_shards? {
        global_open_orders = checked_add(global_open_orders, total, "global open-order count")?;
        for (owner, count) in owners {
            let account_count = account_open_orders.entry(owner).or_default();
            *account_count = checked_add(*account_count, count, "account open-order count")?;
            let auction_count = actual_auction_counts.entry(owner).or_default();
            *auction_count = checked_add(*auction_count, count, "auction order count")?;
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

fn checked_add(value: usize, increment: usize, label: &str) -> Result<usize, StepFatal> {
    value
        .checked_add(increment)
        .ok_or_else(|| invariant(&format!("{label} overflow")))
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
