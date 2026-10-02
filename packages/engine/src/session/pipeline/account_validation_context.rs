use super::{AccountValidationContext, StepFatal, StockValidation};
use crate::{GameSession, SecurityCategory, StockCode};
use rayon::prelude::*;
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct StockValidationFacts {
    pub(super) category: SecurityCategory,
    pub(super) market_buy_protective_price: crate::Money,
    pub(super) market_sell_protective_price: crate::Money,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct AccountValidationFacts {
    pub(super) stocks: BTreeMap<StockCode, StockValidationFacts>,
}

/// Captures the real post-P0 facts consumed by P3 without deriving board semantics
/// from a stock-code prefix or inventing protective prices for market orders.
pub(super) fn build_account_validation_context(
    session: &GameSession,
) -> Result<AccountValidationContext, StepFatal> {
    let facts = collect_account_validation_context_facts(session)?;
    AccountValidationContext::new(facts.stocks.into_iter().map(|(code, stock)| {
        (
            code,
            StockValidation::new(
                stock.category,
                stock.market_buy_protective_price,
                stock.market_sell_protective_price,
            ),
        )
    }))
}

pub(super) fn collect_account_validation_context_facts(
    session: &GameSession,
) -> Result<AccountValidationFacts, StepFatal> {
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
            let stock = StockValidationFacts {
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
    Ok(AccountValidationFacts { stocks })
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::account_validation_context".to_owned(),
    }
}
