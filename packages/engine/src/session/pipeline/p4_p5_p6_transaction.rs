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
    p6_transaction::{prepare_p6_transaction, P6TransactionError, P6TransactionOutput},
    retail_projection::RetailProjectionSeen,
    EnvelopeLedger, EnvelopeReceipt, StepFatal,
};
use crate::session::{account_book::AccountBook, account_paged_map::AccountPagedMap};
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
    pub(super) account_patch: BTreeMap<AccountId, Account>,
    pub(super) retail_patch: BTreeMap<AccountId, RetailExperienceState>,
    pub(super) seen: RetailProjectionSeen,
    pub(super) stocks: BTreeMap<StockCode, P4P5P6StockOutput>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) p6: P6TransactionOutput,
}

pub(super) struct P6ApplicationContext<'receipt> {
    market_minute: u64,
    preceding_receipts: &'receipt [EnvelopeReceipt],
    t1_enabled: bool,
}

impl<'receipt> P6ApplicationContext<'receipt> {
    pub(super) const fn new(
        market_minute: u64,
        preceding_receipts: &'receipt [EnvelopeReceipt],
        t1_enabled: bool,
    ) -> Self {
        Self {
            market_minute,
            preceding_receipts,
            t1_enabled,
        }
    }
}

/// Applies the P4 continuous-worker outputs through P5 then P6 on private candidates.
///
/// Stock identity comes from each worker's resulting `Market`, never its position in `workers`.
/// P5 remains the only receipt-index authority, while P6 remains the only account-settlement and
/// retail-projection authority.
pub(super) fn apply_p4_p5_p6_transaction(
    ledger: &EnvelopeLedger,
    accounts: &AccountBook,
    retail_experience: &AccountPagedMap<crate::RetailExperienceState>,
    seen: &RetailProjectionSeen,
    market_minute: u64,
    workers: Vec<ContinuousStockOutput>,
    t1_enabled: bool,
) -> Result<P4P5P6TransactionOutput, P4P5P6TransactionError> {
    apply_p4_p5_p6_transaction_with_preceding_receipts(
        ledger,
        accounts,
        retail_experience,
        seen,
        workers,
        P6ApplicationContext::new(market_minute, &[], t1_enabled),
    )
}

/// Runs the final P6 once over the already-applied PreSeal prefix followed by
/// the receipts produced by this P5 batch. PreSeal receipts must not be sent
/// through P5 again: their ledger transitions and indices were committed by
/// P0 before the immutable allocation snapshot was captured.
pub(super) fn apply_p4_p5_p6_transaction_with_preceding_receipts(
    ledger: &EnvelopeLedger,
    accounts: &AccountBook,
    retail_experience: &AccountPagedMap<crate::RetailExperienceState>,
    seen: &RetailProjectionSeen,
    workers: Vec<ContinuousStockOutput>,
    context: P6ApplicationContext<'_>,
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

    crate::verification_evidence::enter_phase(super::TickPhase::ReceiptAggregation);
    let mut ledger_candidate = ledger.clone();
    let receipts = apply_receipt_transaction(
        &mut ledger_candidate,
        created_envelopes,
        worker_batches,
        terminal_keys,
    )
    .map_err(P4P5P6TransactionError::P5)?;

    crate::verification_evidence::enter_phase(super::TickPhase::SettlementShadow);
    let mut p6_receipts = Vec::with_capacity(
        context
            .preceding_receipts
            .len()
            .checked_add(receipts.len())
            .ok_or_else(|| {
                P4P5P6TransactionError::P6(P6TransactionError::Settlement(invariant(
                    "combined P6 receipt count overflow",
                )))
            })?,
    );
    p6_receipts.extend_from_slice(context.preceding_receipts);
    p6_receipts.extend_from_slice(&receipts);
    let prepared = prepare_p6_transaction(
        accounts,
        retail_experience,
        seen,
        context.market_minute,
        &p6_receipts,
        context.t1_enabled,
    )
    .map_err(P4P5P6TransactionError::P6)?;

    Ok(P4P5P6TransactionOutput {
        ledger: ledger_candidate,
        account_patch: prepared.account_patch,
        retail_patch: prepared.retail_patch,
        seen: prepared.seen,
        stocks,
        receipts,
        p6: prepared.output,
    })
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p4_p5_p6_transaction".to_owned(),
    }
}
