use super::{P3StockValidation, P3ValidationContext, StepFatal};
use crate::{GameSession, SecurityCategory, StockCode};
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
}

/// Captures the real post-P0 facts consumed by P3 without deriving board semantics
/// from a stock-code prefix or inventing protective prices for market orders.
pub(super) fn build_p3_validation_context(
    session: &GameSession,
) -> Result<P3ValidationContext, StepFatal> {
    let facts = collect_p3_context_facts(session)?;
    P3ValidationContext::new(facts.stocks.into_iter().map(|(code, stock)| {
        (
            code,
            P3StockValidation::new(
                stock.category,
                stock.market_buy_protective_price,
                stock.market_sell_protective_price,
            ),
        )
    }))
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
    let stocks = session
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
            Ok((code.clone(), stock))
        })
        .collect::<Result<BTreeMap<_, _>, StepFatal>>()?;
    Ok(P3ContextFacts { stocks })
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p3_context".to_owned(),
    }
}
