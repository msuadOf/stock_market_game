//! Per-stock continuous P4 shadows that survive adaptive route boundaries.
//!
//! The coordinator is private tick state. It is initialized once from the post-P0 candidate,
//! applies every P3-accepted operation exactly once, and only exposes a consuming `finish` seam.
//! P5/P6/P7 therefore see one accumulated P4 outbox after all adaptive routes have drained.

#[cfg(test)]
use super::ContinuousEnvelopeSnapshot;
#[cfg(feature = "simulation-diagnostics")]
use super::ContinuousOperationQuotes;
use super::{
    process_continuous_stock_step, process_continuous_stock_step_with_ledger,
    ContinuousAcceptanceQuote, ContinuousCancelFact, ContinuousCancelRejection,
    ContinuousExecutionFact, ContinuousExecutionOutcome, ContinuousPlaceFact, ContinuousStockInput,
    ContinuousStockOutput, ContinuousTradeFact,
};
use crate::session::pipeline::{
    EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, P2CandidateKey, P3ValidatedOperation, StepFatal,
};
use crate::{GameConfig, Market, Money, StockCode, TradingPhase};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(in crate::session::pipeline) struct ContinuousStockProjection {
    pub(in crate::session::pipeline) market: Market,
    pub(in crate::session::pipeline) acceptance_quotes: BTreeMap<u64, ContinuousAcceptanceQuote>,
}

/// Only the facts produced by this call to `apply_round`.
#[derive(Debug)]
pub(in crate::session::pipeline) struct ContinuousExecutionRound {
    pub(in crate::session::pipeline) facts: Vec<ContinuousExecutionFact>,
    pub(in crate::session::pipeline) receipts: Vec<EnvelopeReceipt>,
    pub(in crate::session::pipeline) trades: Vec<ContinuousTradeFact>,
    pub(in crate::session::pipeline) projections: BTreeMap<StockCode, ContinuousStockProjection>,
    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session::pipeline) operation_quotes: BTreeMap<u64, ContinuousOperationQuotes>,
}

/// Accumulated P4 output after the caller has drained every adaptive route.
pub(in crate::session::pipeline) struct IncrementalContinuousStockFinish {
    pub(in crate::session::pipeline) workers: Vec<ContinuousStockOutput>,
    pub(in crate::session::pipeline) prices: BTreeMap<StockCode, ContinuousClosingPrice>,
    pub(in crate::session::pipeline) execution_facts: Vec<ContinuousExecutionFact>,
    /// P4 business rejections that have no authoritative stock worker, currently only an
    /// unknown-stock cancellation accepted by P3 without consulting the stock map.
    pub(in crate::session::pipeline) detached_facts: Vec<ContinuousExecutionFact>,
}

/// Freeze the final operation book before an optional DayEnd clears its depth.
pub(in crate::session::pipeline) struct ContinuousClosingPrice {
    pub(in crate::session::pipeline) last: Money,
    pub(in crate::session::pipeline) bids: Vec<(Money, u64)>,
    pub(in crate::session::pipeline) asks: Vec<(Money, u64)>,
}

#[derive(Debug)]
pub(in crate::session::pipeline) struct IncrementalContinuousStockCoordinator {
    phase: Option<TradingPhase>,
    stocks: BTreeMap<StockCode, IncrementalContinuousStockShadow>,
    detached_facts: Vec<ContinuousExecutionFact>,
    seen_candidate_keys: BTreeSet<P2CandidateKey>,
    seen_sealed_indices: BTreeSet<u64>,
    applied_operation_count: usize,
    failed: bool,
}

#[derive(Clone, Debug)]
struct IncrementalContinuousStockShadow {
    phase: TradingPhase,
    market: Market,
    ledger: EnvelopeLedger,
    config: GameConfig,
    next_trade_event_index: u64,
    created_envelopes: Vec<crate::session::pipeline::Envelope>,
    receipts: Vec<EnvelopeReceipt>,
    terminal_keys: Vec<EnvelopeKey>,
    trades: Vec<ContinuousTradeFact>,
    place_facts: Vec<ContinuousPlaceFact>,
    cancel_facts: Vec<ContinuousCancelFact>,
    execution_facts: Vec<ContinuousExecutionFact>,
}

struct StockRoundResult {
    code: StockCode,
    shadow: IncrementalContinuousStockShadow,
    facts: Vec<ContinuousExecutionFact>,
    receipts: Vec<EnvelopeReceipt>,
    trades: Vec<ContinuousTradeFact>,
    acceptance_quotes: BTreeMap<u64, ContinuousAcceptanceQuote>,
    #[cfg(feature = "simulation-diagnostics")]
    operation_quotes: BTreeMap<u64, ContinuousOperationQuotes>,
}

impl IncrementalContinuousStockCoordinator {
    pub(in crate::session::pipeline) fn detached(phase: TradingPhase) -> Self {
        Self {
            phase: Some(phase),
            stocks: BTreeMap::new(),
            detached_facts: Vec::new(),
            seen_candidate_keys: BTreeSet::new(),
            seen_sealed_indices: BTreeSet::new(),
            applied_operation_count: 0,
            failed: false,
        }
    }

    pub(in crate::session::pipeline) fn from_post_p0(
        inputs: Vec<ContinuousStockInput>,
    ) -> Result<Self, StepFatal> {
        let phase = inputs.first().map(|input| input.phase);
        if inputs.iter().any(|input| Some(input.phase) != phase) {
            return Err(invariant(
                "incremental P4 stock shadows disagree on the trading phase",
            ));
        }
        let mut stocks = BTreeMap::new();
        for input in inputs {
            if !input.operations.is_empty() {
                return Err(invariant(
                    "incremental P4 initialization included sealed operations",
                ));
            }
            let code = input.market.code().clone();
            if stocks.contains_key(&code) {
                return Err(invariant(
                    "incremental P4 initialized one stock shadow more than once",
                ));
            }
            let phase = input.phase;
            let config = input.config.clone();
            let initialized = process_continuous_stock_step(input, 0)?;
            if !initialized.execution_facts.is_empty()
                || !initialized.output.receipts.is_empty()
                || !initialized.output.created_envelopes.is_empty()
            {
                return Err(invariant(
                    "operation-free P4 initialization produced an outbox",
                ));
            }
            stocks.insert(
                code,
                IncrementalContinuousStockShadow {
                    phase,
                    market: initialized.output.market,
                    ledger: initialized.ledger,
                    config,
                    next_trade_event_index: initialized.next_trade_event_index,
                    created_envelopes: Vec::new(),
                    receipts: Vec::new(),
                    terminal_keys: Vec::new(),
                    trades: Vec::new(),
                    place_facts: Vec::new(),
                    cancel_facts: Vec::new(),
                    execution_facts: Vec::new(),
                },
            );
        }
        Ok(Self {
            phase,
            stocks,
            detached_facts: Vec::new(),
            seen_candidate_keys: BTreeSet::new(),
            seen_sealed_indices: BTreeSet::new(),
            applied_operation_count: 0,
            failed: false,
        })
    }

    /// Applies one ready route round to the discardable tick candidate. A typed
    /// failure invalidates this coordinator; the caller drops the entire tick
    /// candidate, so copying every touched order book to support a retry here
    /// would duplicate the authority rollback boundary.
    pub(in crate::session::pipeline) fn apply_round(
        &mut self,
        operations: Vec<P3ValidatedOperation>,
    ) -> Result<ContinuousExecutionRound, StepFatal> {
        if self.failed {
            return Err(invariant(
                "failed P4 coordinator cannot accept another round",
            ));
        }
        let result = self.apply_round_owned(operations);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn apply_round_owned(
        &mut self,
        operations: Vec<P3ValidatedOperation>,
    ) -> Result<ContinuousExecutionRound, StepFatal> {
        validate_new_operation_identities(self, &operations)?;
        let next_operation_count = self
            .applied_operation_count
            .checked_add(operations.len())
            .ok_or_else(|| invariant("incremental P4 operation count overflow"))?;

        let mut grouped = BTreeMap::<StockCode, Vec<P3ValidatedOperation>>::new();
        let mut detached = Vec::new();
        let mut new_candidate_keys = BTreeSet::new();
        let mut new_sealed_indices = BTreeSet::new();
        for operation in operations {
            let candidate_key = operation.candidate_key().clone();
            let sealed_index = operation.sealed_index();
            new_candidate_keys.insert(candidate_key.clone());
            new_sealed_indices.insert(sealed_index);
            let code = operation_code(&operation).clone();
            if self.stocks.contains_key(&code) {
                grouped.entry(code).or_default().push(operation);
                continue;
            }
            match operation {
                P3ValidatedOperation::Cancel {
                    candidate_key,
                    sealed_index,
                    account,
                    code,
                    order_id,
                } => detached.push(ContinuousExecutionFact {
                    candidate_key,
                    sealed_index,
                    allocated_order_id: None,
                    outcome: ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
                        sealed_index,
                        account,
                        code,
                        order_id,
                        reason: if self.phase == Some(TradingPhase::PreOpen) {
                            ContinuousCancelRejection::AuctionOrderNotCancelable
                        } else {
                            ContinuousCancelRejection::UnknownStock
                        },
                    }),
                }),
                P3ValidatedOperation::Place(_) => {
                    return Err(invariant("P3 accepted a place for an unknown stock"));
                }
            }
        }

        let work = grouped
            .into_iter()
            .map(|(code, operations)| {
                let shadow = self
                    .stocks
                    .remove(&code)
                    .ok_or_else(|| invariant("P4 stock partition lost its initialized shadow"))?;
                Ok((code, shadow, operations))
            })
            .collect::<Result<Vec<_>, StepFatal>>()?;
        #[cfg(any(test, feature = "verification-harness"))]
        let work = {
            let mut work = work;
            crate::session::pipeline::executor_perturbation::reorder(
                crate::session::pipeline::ExecutorBoundary::P4ContinuousStockShards,
                &mut work,
                |(code, _, operations)| (code.0.clone(), operations.len()),
            );
            work
        };
        let results = work
            .into_par_iter()
            .map(|(code, shadow, operations)| {
                let identity = code.clone();
                let result = apply_stock_round(code, shadow, operations);
                (identity, result)
            })
            .collect::<Vec<_>>();
        #[cfg(any(test, feature = "verification-harness"))]
        let results = {
            let mut results = results;
            crate::session::pipeline::executor_perturbation::reorder(
                crate::session::pipeline::ExecutorBoundary::P4ContinuousWorkerResults,
                &mut results,
                |(code, result)| {
                    (
                        code.0.clone(),
                        result.as_ref().map_or(0, |result| result.facts.len()),
                    )
                },
            );
            results
        };

        // Rayon preserves indexed input order, but the caller may deliver completed work in a
        // different order. Select errors and merge successful outputs by stock identity.
        let mut results = results;
        results.sort_by(|left, right| left.0.cmp(&right.0));
        let mut facts = detached.clone();
        let mut receipts = Vec::new();
        let mut trades = Vec::new();
        let mut projections = BTreeMap::new();
        let mut stock_updates = Vec::with_capacity(results.len());
        #[cfg(feature = "simulation-diagnostics")]
        let mut operation_quotes = BTreeMap::new();
        for (_, result) in results {
            let result = result?;
            facts.extend(result.facts);
            receipts.extend(result.receipts);
            trades.extend(result.trades);
            #[cfg(feature = "simulation-diagnostics")]
            for (sealed_index, quotes) in result.operation_quotes {
                if operation_quotes.insert(sealed_index, quotes).is_some() {
                    return Err(invariant(
                        "incremental P4 round contains duplicate operation quote identity",
                    ));
                }
            }
            projections.insert(
                result.code.clone(),
                ContinuousStockProjection {
                    market: result.shadow.market.clone(),
                    acceptance_quotes: result.acceptance_quotes,
                },
            );
            stock_updates.push((result.code, result.shadow));
        }

        validate_round_identities(&facts, &receipts)?;
        self.stocks.extend(stock_updates);
        self.detached_facts.extend(detached);
        self.seen_candidate_keys.extend(new_candidate_keys);
        self.seen_sealed_indices.extend(new_sealed_indices);
        self.applied_operation_count = next_operation_count;
        Ok(ContinuousExecutionRound {
            facts,
            receipts,
            trades,
            projections,
            #[cfg(feature = "simulation-diagnostics")]
            operation_quotes,
        })
    }

    pub(in crate::session::pipeline) fn finish(
        self,
    ) -> Result<IncrementalContinuousStockFinish, StepFatal> {
        self.finish_for_tick(false)
    }

    /// The consuming boundary is reached only after every continuation has drained.
    pub(in crate::session::pipeline) fn finish_for_tick(
        self,
        ends_day: bool,
    ) -> Result<IncrementalContinuousStockFinish, StepFatal> {
        if self.failed {
            return Err(invariant("failed P4 coordinator cannot finish a tick"));
        }
        let mut workers = Vec::with_capacity(self.stocks.len());
        let mut prices = BTreeMap::new();
        let mut execution_facts = self.detached_facts.clone();
        let mut execution_count = self.detached_facts.len();
        for (_, mut stock) in self.stocks {
            prices.insert(
                stock.market.code().clone(),
                ContinuousClosingPrice {
                    last: stock.market.last_price(),
                    bids: stock.market.bid_depth_limited(5),
                    asks: stock.market.ask_depth_limited(5),
                },
            );
            if ends_day {
                for (ordinal, (key, envelope)) in stock.ledger.iter().enumerate() {
                    let source = u32::try_from(ordinal)
                        .map_err(|_| invariant("continuous DayEnd source index overflow"))?;
                    stock.receipts.push(
                        crate::session::pipeline::stock_auction::day_end_release_receipt(
                            envelope, source,
                        )?,
                    );
                    stock.terminal_keys.push(key.clone());
                }
                stock.market.end_of_day();
            }
            execution_count = execution_count
                .checked_add(stock.execution_facts.len())
                .ok_or_else(|| invariant("incremental P4 fact count overflow"))?;
            execution_facts.append(&mut stock.execution_facts);
            workers.push(ContinuousStockOutput {
                market: stock.market,
                created_envelopes: stock.created_envelopes,
                receipts: stock.receipts,
                terminal_keys: stock.terminal_keys,
                trades: stock.trades,
                place_facts: stock.place_facts,
                cancel_facts: stock.cancel_facts,
            });
        }
        if execution_count != self.applied_operation_count {
            return Err(invariant(
                "incremental P4 did not produce exactly one fact per operation",
            ));
        }
        Ok(IncrementalContinuousStockFinish {
            workers,
            prices,
            execution_facts,
            detached_facts: self.detached_facts,
        })
    }
}

fn apply_stock_round(
    code: StockCode,
    mut shadow: IncrementalContinuousStockShadow,
    operations: Vec<P3ValidatedOperation>,
) -> Result<StockRoundResult, StepFatal> {
    let step = process_continuous_stock_step_with_ledger(
        ContinuousStockInput {
            phase: shadow.phase,
            market: shadow.market,
            envelopes: Vec::new(),
            operations,
            config: shadow.config.clone(),
        },
        shadow.next_trade_event_index,
        shadow.ledger,
    )?;
    let output = step.output;
    if step.execution_facts.len()
        != output
            .place_facts
            .len()
            .saturating_add(output.cancel_facts.len())
    {
        return Err(invariant(
            "stock round typed-fact count disagrees with operation outcomes",
        ));
    }
    let facts = step.execution_facts.clone();
    let receipts = output.receipts.clone();
    let trades = output.trades.clone();

    shadow.market = output.market;
    shadow.ledger = step.ledger;
    shadow.next_trade_event_index = step.next_trade_event_index;
    shadow.created_envelopes.extend(output.created_envelopes);
    shadow.receipts.extend(output.receipts);
    shadow.terminal_keys.extend(output.terminal_keys);
    shadow.trades.extend(output.trades);
    shadow.place_facts.extend(output.place_facts);
    shadow.cancel_facts.extend(output.cancel_facts);
    shadow.execution_facts.extend(step.execution_facts);

    Ok(StockRoundResult {
        code,
        shadow,
        facts,
        receipts,
        trades,
        acceptance_quotes: step.acceptance_quotes,
        #[cfg(feature = "simulation-diagnostics")]
        operation_quotes: step.operation_quotes,
    })
}

fn validate_new_operation_identities(
    coordinator: &IncrementalContinuousStockCoordinator,
    operations: &[P3ValidatedOperation],
) -> Result<(), StepFatal> {
    let mut candidates = BTreeSet::new();
    let mut sealed = BTreeSet::new();
    for operation in operations {
        if coordinator
            .seen_candidate_keys
            .contains(operation.candidate_key())
            || !candidates.insert(operation.candidate_key().clone())
        {
            return Err(invariant("incremental P4 replayed a candidate key"));
        }
        if coordinator
            .seen_sealed_indices
            .contains(&operation.sealed_index())
            || !sealed.insert(operation.sealed_index())
        {
            return Err(invariant("incremental P4 replayed a sealed identity"));
        }
    }
    Ok(())
}

fn validate_round_identities(
    facts: &[ContinuousExecutionFact],
    receipts: &[EnvelopeReceipt],
) -> Result<(), StepFatal> {
    // Each stock worker emits facts and receipts in its own execution order. Sorting those
    // by an audit identity would turn that identity back into a business clock.
    let fact_ids = facts
        .iter()
        .map(|fact| fact.sealed_index)
        .collect::<BTreeSet<_>>();
    if fact_ids.len() != facts.len() {
        return Err(invariant(
            "incremental P4 round contains duplicate typed fact identities",
        ));
    }
    let receipt_ids = receipts
        .iter()
        .map(|receipt| &receipt.local_key)
        .collect::<BTreeSet<_>>();
    if receipt_ids.len() != receipts.len() {
        return Err(invariant(
            "incremental P4 round contains duplicate receipt identities",
        ));
    }
    Ok(())
}

fn operation_code(operation: &P3ValidatedOperation) -> &StockCode {
    match operation {
        P3ValidatedOperation::Place(draft) => draft.code(),
        P3ValidatedOperation::Cancel { code, .. } => code,
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::incremental_continuous_stock_shadow".to_owned(),
    }
}

#[cfg(test)]
#[path = "incremental_continuous_stock_shadow_tests.rs"]
mod tests;
