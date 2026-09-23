//! Detached atomic candidate for the continuous-worker, receipt, and settlement stages.
//!
//! This is deliberately not a `GameSession` integration point. It consumes the owned P4 worker
//! outputs, runs P5 and P6 against private authority shadows, and returns one candidate for a
//! later P9 commit. No input authority is mutated here.

use super::{
    p4_continuous::{
        ContinuousCancelFact, ContinuousPlaceFact, ContinuousStockOutput, ContinuousTradeFact,
    },
    p5_receipts::apply_receipt_transaction,
    p6_transaction::{apply_p6_transaction, P6TransactionError, P6TransactionOutput},
    retail_projection::RetailProjectionSeen,
    EnvelopeLedger, EnvelopeReceipt, StepFatal,
};
use crate::{Account, AccountId, Market, RetailExperienceState, StockCode};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub(super) enum P4P5P6TransactionError {
    #[error("P4 produced more than one continuous worker output for stock {code:?}")]
    DuplicateStockWorker { code: StockCode },
    #[error("P5 receipt transaction failed: {0}")]
    P5(#[source] StepFatal),
    #[error("P6 settlement transaction failed: {0}")]
    P6(#[source] P6TransactionError),
}

/// P4 facts and the resulting market owned by one deterministically identified stock worker.
pub(super) struct P4P5P6StockOutput {
    pub(super) market: Market,
    pub(super) trades: Vec<ContinuousTradeFact>,
    pub(super) place_facts: Vec<ContinuousPlaceFact>,
    pub(super) cancel_facts: Vec<ContinuousCancelFact>,
}

/// A single detached candidate spanning P4, P5, and P6.
///
/// The returned containers must be installed together by the future P9 commit. Returning a
/// candidate instead of mutating the inputs prevents a successful P5 ledger update from becoming
/// externally visible when P6 fails.
pub(super) struct P4P5P6TransactionOutput {
    pub(super) ledger: EnvelopeLedger,
    pub(super) accounts: BTreeMap<AccountId, Account>,
    pub(super) retail_experience: BTreeMap<AccountId, RetailExperienceState>,
    pub(super) seen: RetailProjectionSeen,
    pub(super) stocks: BTreeMap<StockCode, P4P5P6StockOutput>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) p6: P6TransactionOutput,
}

/// Applies the P4 continuous-worker outputs through P5 then P6 on private candidates.
///
/// Stock identity comes from each worker's resulting `Market`, never its position in `workers`.
/// P5 remains the only receipt-index authority, while P6 remains the only account-settlement and
/// retail-projection authority.
pub(super) fn apply_p4_p5_p6_transaction(
    ledger: &EnvelopeLedger,
    accounts: &BTreeMap<AccountId, Account>,
    retail_experience: &BTreeMap<AccountId, RetailExperienceState>,
    seen: &RetailProjectionSeen,
    market_minute: u64,
    workers: Vec<ContinuousStockOutput>,
    t1_enabled: bool,
) -> Result<P4P5P6TransactionOutput, P4P5P6TransactionError> {
    let mut stocks = BTreeMap::new();
    let mut created_envelopes = Vec::new();
    let mut worker_batches = Vec::new();
    let mut terminal_keys = Vec::new();

    for worker in workers {
        let code = worker.market.code().clone();
        if stocks.contains_key(&code) {
            return Err(P4P5P6TransactionError::DuplicateStockWorker { code });
        }
        created_envelopes.extend(worker.created_envelopes);
        worker_batches.push(worker.receipts);
        terminal_keys.extend(worker.terminal_keys);
        stocks.insert(
            code,
            P4P5P6StockOutput {
                market: worker.market,
                trades: worker.trades,
                place_facts: worker.place_facts,
                cancel_facts: worker.cancel_facts,
            },
        );
    }

    let mut ledger_candidate = ledger.clone();
    let receipts = apply_receipt_transaction(
        &mut ledger_candidate,
        created_envelopes,
        worker_batches,
        terminal_keys,
    )
    .map_err(P4P5P6TransactionError::P5)?;

    let mut account_candidate = clone_accounts(accounts)
        .map_err(|error| P4P5P6TransactionError::P6(P6TransactionError::Settlement(error)))?;
    let mut retail_candidate = retail_experience.clone();
    let mut seen_candidate = seen.clone();
    let p6 = apply_p6_transaction(
        &mut account_candidate,
        &mut retail_candidate,
        &mut seen_candidate,
        market_minute,
        &receipts,
        t1_enabled,
    )
    .map_err(P4P5P6TransactionError::P6)?;

    Ok(P4P5P6TransactionOutput {
        ledger: ledger_candidate,
        accounts: account_candidate,
        retail_experience: retail_candidate,
        seen: seen_candidate,
        stocks,
        receipts,
        p6,
    })
}

fn clone_accounts(
    accounts: &BTreeMap<AccountId, Account>,
) -> Result<BTreeMap<AccountId, Account>, StepFatal> {
    accounts
        .iter()
        .map(|(account_id, account)| {
            account
                .clone_for_shadow()
                .map(|shadow| (*account_id, shadow))
                .map_err(|error| StepFatal::InvariantViolation {
                    description: format!("could not clone P4-P6 account shadow: {error}"),
                    location: "pipeline::p4_p5_p6_transaction".to_owned(),
                })
        })
        .collect()
}
