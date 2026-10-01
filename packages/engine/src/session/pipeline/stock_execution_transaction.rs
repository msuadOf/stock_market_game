//! Detached atomic candidate for the continuous-worker, receipt, and settlement stages.
//!
//! This is deliberately not a `GameSession` integration point. It consumes the owned P4 worker
//! outputs, runs ReceiptAggregation and Settlement against private authority shadows, and returns one candidate for a
//! later P9 commit. No input authority is mutated here.

use super::{
    account_settlement::{
        prepare_settlement_transaction, SettlementTransactionError, SettlementTransactionOutput,
    },
    continuous_matching::{
        ContinuousCancelFact, ContinuousPlaceFact, ContinuousStockOutput, ContinuousTradeFact,
    },
    receipt_aggregation::apply_owned_receipt_transaction,
    retail_projection::RetailProjectionSeen,
    EnvelopeLedger, EnvelopeReceipt, StepFatal,
};
use crate::session::{account_book::AccountBook, account_paged_map::AccountPagedMap};
use crate::{Account, AccountId, Market, RetailExperienceState, StockCode};
use std::collections::BTreeMap;

#[derive(Debug, thiserror::Error)]
pub(super) enum StockExecutionTransactionError {
    #[error("P4 produced more than one continuous worker output for stock {code:?}")]
    DuplicateStockWorker { code: StockCode },
    #[error("P5 receipt transaction failed: {0}")]
    ReceiptAggregation(#[source] StepFatal),
    #[error("P6 settlement transaction failed: {0}")]
    Settlement(#[source] SettlementTransactionError),
}

/// P4 facts and the resulting market owned by one deterministically identified stock worker.
pub(super) struct StockExecutionOutput {
    pub(super) market: Market,
    pub(super) trades: Vec<ContinuousTradeFact>,
    pub(super) place_facts: Vec<ContinuousPlaceFact>,
    pub(super) cancel_facts: Vec<ContinuousCancelFact>,
}

/// A single detached candidate spanning P4, ReceiptAggregation, and Settlement.
///
/// The returned containers must be installed together by the future P9 commit. Returning a
/// candidate instead of mutating the inputs prevents a successful ReceiptAggregation ledger update from becoming
/// externally visible when Settlement fails.
pub(super) struct StockExecutionTransactionOutput {
    pub(super) ledger: EnvelopeLedger,
    pub(super) account_patch: BTreeMap<AccountId, Account>,
    pub(super) retail_patch: BTreeMap<AccountId, RetailExperienceState>,
    pub(super) seen: RetailProjectionSeen,
    pub(super) stocks: BTreeMap<StockCode, StockExecutionOutput>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) settlement: SettlementTransactionOutput,
}

pub(super) struct SettlementApplicationContext<'receipt> {
    market_minute: u64,
    preceding_receipts: &'receipt [EnvelopeReceipt],
    t1_enabled: bool,
}

impl<'receipt> SettlementApplicationContext<'receipt> {
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

/// Applies the P4 continuous-worker outputs through ReceiptAggregation then Settlement on private candidates.
///
/// Stock identity comes from each worker's resulting `Market`, never its position in `workers`.
/// ReceiptAggregation remains the only receipt-index authority, while Settlement remains the only account-settlement and
/// retail-projection authority.
pub(super) fn apply_stock_execution_transaction(
    ledger: &EnvelopeLedger,
    accounts: &AccountBook,
    retail_experience: &AccountPagedMap<crate::RetailExperienceState>,
    seen: &RetailProjectionSeen,
    market_minute: u64,
    workers: Vec<ContinuousStockOutput>,
    t1_enabled: bool,
) -> Result<StockExecutionTransactionOutput, StockExecutionTransactionError> {
    apply_stock_execution_transaction_with_preceding_receipts(
        ledger,
        accounts,
        retail_experience,
        seen,
        workers,
        SettlementApplicationContext::new(market_minute, &[], t1_enabled),
    )
}

/// Runs the final Settlement once over the already-applied PreSeal prefix followed by
/// the receipts produced by this ReceiptAggregation batch. PreSeal receipts must not be sent
/// through ReceiptAggregation again: their ledger transitions and indices were committed by
/// P0 before the immutable allocation snapshot was captured.
pub(super) fn apply_stock_execution_transaction_with_preceding_receipts(
    ledger: &EnvelopeLedger,
    accounts: &AccountBook,
    retail_experience: &AccountPagedMap<crate::RetailExperienceState>,
    seen: &RetailProjectionSeen,
    workers: Vec<ContinuousStockOutput>,
    context: SettlementApplicationContext<'_>,
) -> Result<StockExecutionTransactionOutput, StockExecutionTransactionError> {
    let mut stocks = BTreeMap::new();
    let mut created_envelopes = Vec::new();
    let mut worker_batches = Vec::new();
    let mut terminal_keys = Vec::new();

    for worker in workers {
        let code = worker.market.code().clone();
        if stocks.contains_key(&code) {
            return Err(StockExecutionTransactionError::DuplicateStockWorker { code });
        }
        created_envelopes.extend(worker.created_envelopes);
        worker_batches.push(worker.receipts);
        terminal_keys.extend(worker.terminal_keys);
        stocks.insert(
            code,
            StockExecutionOutput {
                market: worker.market,
                trades: worker.trades,
                place_facts: worker.place_facts,
                cancel_facts: worker.cancel_facts,
            },
        );
    }

    crate::verification_evidence::enter_phase(super::TickPhase::ReceiptAggregation);
    let (ledger_candidate, receipts) = apply_owned_receipt_transaction(
        ledger.clone(),
        created_envelopes,
        worker_batches,
        terminal_keys,
    )
    .map_err(StockExecutionTransactionError::ReceiptAggregation)?;

    crate::verification_evidence::enter_phase(super::TickPhase::SettlementShadow);
    let mut settlement_receipts = Vec::with_capacity(
        context
            .preceding_receipts
            .len()
            .checked_add(receipts.len())
            .ok_or_else(|| {
                StockExecutionTransactionError::Settlement(SettlementTransactionError::Settlement(
                    invariant("combined P6 receipt count overflow"),
                ))
            })?,
    );
    settlement_receipts.extend_from_slice(context.preceding_receipts);
    settlement_receipts.extend_from_slice(&receipts);
    let prepared = prepare_settlement_transaction(
        accounts,
        retail_experience,
        seen,
        context.market_minute,
        &settlement_receipts,
        context.t1_enabled,
    )
    .map_err(StockExecutionTransactionError::Settlement)?;

    Ok(StockExecutionTransactionOutput {
        ledger: ledger_candidate,
        account_patch: prepared.account_patch,
        retail_patch: prepared.retail_patch,
        seen: prepared.seen,
        stocks,
        receipts,
        settlement: prepared.output,
    })
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::stock_execution_transaction".to_owned(),
    }
}
