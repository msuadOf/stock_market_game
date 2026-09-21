//! Per-stock continuous P4 shadows that survive adaptive route boundaries.
//!
//! The coordinator is private tick state. It is initialized once from the post-P0 candidate,
//! applies every P3-accepted operation exactly once, and only exposes a consuming `finish` seam.
//! P5/P6/P7 therefore see one accumulated P4 outbox after all adaptive routes have drained.

use super::{
    ledger_snapshots, process_continuous_stock_step, process_continuous_stock_step_with_ledger,
    ContinuousAcceptanceQuote, ContinuousCancelFact, ContinuousCancelRejection,
    ContinuousEnvelopeSnapshot, ContinuousExecutionFact, ContinuousExecutionOutcome,
    ContinuousPlaceFact, ContinuousStockInput, ContinuousStockOutput, ContinuousTradeFact,
};
use crate::session::pipeline::{
    EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, P2CandidateKey, P3ValidatedOperation,
    ReceiptKind, StepFatal,
};
use crate::{AccountId, GameConfig, Market, OrderId, StockCode, TradingPhase};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::session::pipeline) enum ContinuousOpenOrderDeltaKind {
    Opened,
    ClosedByFill,
    ClosedByCancel,
}

/// A quantity-slot update caused by one P4 operation.
///
/// These deltas update only P3's global/per-account open-order constraint state. They never make
/// sealed-batch cash or shares reusable in the current tick.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session::pipeline) struct ContinuousOpenOrderDelta {
    pub(in crate::session::pipeline) candidate_key: P2CandidateKey,
    pub(in crate::session::pipeline) sealed_index: u64,
    pub(in crate::session::pipeline) account: AccountId,
    pub(in crate::session::pipeline) stock: StockCode,
    pub(in crate::session::pipeline) order_id: OrderId,
    pub(in crate::session::pipeline) delta: i8,
    pub(in crate::session::pipeline) kind: ContinuousOpenOrderDeltaKind,
}

#[derive(Clone, Debug)]
pub(in crate::session::pipeline) struct ContinuousStockProjection {
    pub(in crate::session::pipeline) market: Market,
    pub(in crate::session::pipeline) live_envelopes: Vec<ContinuousEnvelopeSnapshot>,
    pub(in crate::session::pipeline) acceptance_quotes: BTreeMap<u64, ContinuousAcceptanceQuote>,
}

/// Only the facts produced by this call to `apply_round`.
#[derive(Debug)]
pub(in crate::session::pipeline) struct ContinuousExecutionRound {
    pub(in crate::session::pipeline) facts: Vec<ContinuousExecutionFact>,
    pub(in crate::session::pipeline) receipts: Vec<EnvelopeReceipt>,
    pub(in crate::session::pipeline) trades: Vec<ContinuousTradeFact>,
    pub(in crate::session::pipeline) projections: BTreeMap<StockCode, ContinuousStockProjection>,
    pub(in crate::session::pipeline) open_order_deltas: Vec<ContinuousOpenOrderDelta>,
}

/// Accumulated P4 output after the caller has drained every adaptive route.
pub(in crate::session::pipeline) struct IncrementalContinuousStockFinish {
    pub(in crate::session::pipeline) workers: Vec<ContinuousStockOutput>,
    /// P4 business rejections that have no authoritative stock worker, currently only an
    /// unknown-stock cancellation accepted by P3 without consulting the stock map.
    pub(in crate::session::pipeline) detached_facts: Vec<ContinuousExecutionFact>,
}

#[derive(Clone, Debug)]
pub(in crate::session::pipeline) struct IncrementalContinuousStockCoordinator {
    stocks: BTreeMap<StockCode, IncrementalContinuousStockShadow>,
    detached_facts: Vec<ContinuousExecutionFact>,
    seen_candidate_keys: BTreeSet<P2CandidateKey>,
    seen_sealed_indices: BTreeSet<u64>,
    last_candidate_key: Option<P2CandidateKey>,
    last_sealed_index: Option<u64>,
    applied_operation_count: usize,
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
    open_order_deltas: Vec<ContinuousOpenOrderDelta>,
    acceptance_quotes: BTreeMap<u64, ContinuousAcceptanceQuote>,
}

impl IncrementalContinuousStockCoordinator {
    pub(in crate::session::pipeline) fn from_post_p0(
        inputs: Vec<ContinuousStockInput>,
    ) -> Result<Self, StepFatal> {
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
            let initialized = process_continuous_stock_step(input, false, 0)?;
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
            stocks,
            detached_facts: Vec::new(),
            seen_candidate_keys: BTreeSet::new(),
            seen_sealed_indices: BTreeSet::new(),
            last_candidate_key: None,
            last_sealed_index: None,
            applied_operation_count: 0,
        })
    }

    /// Applies one deterministic route round atomically to private per-stock shadows.
    ///
    /// A typed error leaves `self` at the previous successful route boundary. Independently owned
    /// stock batches execute in parallel; results are canonicalized by explicit identities before
    /// the staged coordinator replaces `self`.
    pub(in crate::session::pipeline) fn apply_round(
        &mut self,
        operations: Vec<P3ValidatedOperation>,
    ) -> Result<ContinuousExecutionRound, StepFatal> {
        let mut staged = self.clone();
        let round = staged.apply_round_in_place(operations)?;
        *self = staged;
        Ok(round)
    }

    fn apply_round_in_place(
        &mut self,
        operations: Vec<P3ValidatedOperation>,
    ) -> Result<ContinuousExecutionRound, StepFatal> {
        validate_new_operation_identities(self, &operations)?;

        let mut grouped = BTreeMap::<StockCode, Vec<P3ValidatedOperation>>::new();
        let mut detached = Vec::new();
        for operation in operations {
            let candidate_key = operation.candidate_key().clone();
            let sealed_index = operation.sealed_index();
            self.seen_candidate_keys.insert(candidate_key.clone());
            self.seen_sealed_indices.insert(sealed_index);
            self.last_candidate_key = Some(candidate_key.clone());
            self.last_sealed_index = Some(sealed_index);
            self.applied_operation_count = self
                .applied_operation_count
                .checked_add(1)
                .ok_or_else(|| invariant("incremental P4 operation count overflow"))?;

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
                        reason: ContinuousCancelRejection::UnknownStock,
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
                let shadow =
                    self.stocks.get(&code).cloned().ok_or_else(|| {
                        invariant("P4 stock partition lost its initialized shadow")
                    })?;
                Ok((code, shadow, operations))
            })
            .collect::<Result<Vec<_>, StepFatal>>()?;
        let results = work
            .into_par_iter()
            .map(|(code, shadow, operations)| apply_stock_round(code, shadow, operations))
            .collect::<Vec<_>>();

        let mut facts = detached.clone();
        let mut receipts = Vec::new();
        let mut trades = Vec::new();
        let mut projections = BTreeMap::new();
        let mut open_order_deltas = Vec::new();
        for result in results {
            let result = result?;
            facts.extend(result.facts);
            receipts.extend(result.receipts);
            trades.extend(result.trades);
            open_order_deltas.extend(result.open_order_deltas);
            projections.insert(
                result.code.clone(),
                ContinuousStockProjection {
                    market: result.shadow.market.clone(),
                    live_envelopes: ledger_snapshots(&result.shadow.ledger),
                    acceptance_quotes: result.acceptance_quotes,
                },
            );
            self.stocks.insert(result.code, result.shadow);
        }
        self.detached_facts.extend(detached);

        canonicalize_round(
            &mut facts,
            &mut receipts,
            &mut trades,
            &mut open_order_deltas,
        )?;
        Ok(ContinuousExecutionRound {
            facts,
            receipts,
            trades,
            projections,
            open_order_deltas,
        })
    }

    pub(in crate::session::pipeline) fn finish(
        self,
    ) -> Result<IncrementalContinuousStockFinish, StepFatal> {
        let mut workers = Vec::with_capacity(self.stocks.len());
        let mut execution_count = self.detached_facts.len();
        for (_, stock) in self.stocks {
            execution_count = execution_count
                .checked_add(stock.execution_facts.len())
                .ok_or_else(|| invariant("incremental P4 fact count overflow"))?;
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
            detached_facts: self.detached_facts,
        })
    }
}

fn apply_stock_round(
    code: StockCode,
    mut shadow: IncrementalContinuousStockShadow,
    operations: Vec<P3ValidatedOperation>,
) -> Result<StockRoundResult, StepFatal> {
    let envelopes = ledger_snapshots(&shadow.ledger);
    let step = process_continuous_stock_step_with_ledger(
        ContinuousStockInput {
            phase: shadow.phase,
            market: shadow.market,
            envelopes,
            operations,
            config: shadow.config.clone(),
        },
        true,
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
    let open_order_deltas = open_order_deltas(&step.execution_facts, &output.receipts)?;
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
        open_order_deltas,
        acceptance_quotes: step.acceptance_quotes,
    })
}

fn open_order_deltas(
    facts: &[ContinuousExecutionFact],
    receipts: &[EnvelopeReceipt],
) -> Result<Vec<ContinuousOpenOrderDelta>, StepFatal> {
    let mut deltas = Vec::new();
    for fact in facts {
        match &fact.outcome {
            ContinuousExecutionOutcome::Place {
                fact:
                    ContinuousPlaceFact::Resting {
                        account,
                        code,
                        order_id,
                        ..
                    },
                ..
            } => deltas.push(delta(
                fact,
                *account,
                code.clone(),
                *order_id,
                1,
                ContinuousOpenOrderDeltaKind::Opened,
            )),
            ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled {
                account,
                code,
                order_id,
                ..
            }) => deltas.push(delta(
                fact,
                *account,
                code.clone(),
                *order_id,
                -1,
                ContinuousOpenOrderDeltaKind::ClosedByCancel,
            )),
            _ => {}
        }

        let incoming_order = fact.allocated_order_id;
        for receipt in receipts.iter().filter(|receipt| {
            receipt.local_key.source().payload() == fact.sealed_index
                && receipt.kind == ReceiptKind::Fill
                && receipt.qty_after == 0
        }) {
            if incoming_order == Some(receipt.envelope.order) {
                continue;
            }
            deltas.push(delta(
                fact,
                receipt.envelope.account,
                receipt.envelope.stock.clone(),
                receipt.envelope.order,
                -1,
                ContinuousOpenOrderDeltaKind::ClosedByFill,
            ));
        }
    }
    deltas.sort_by(delta_cmp_key);
    if deltas
        .windows(2)
        .any(|pair| delta_cmp_key(&pair[0], &pair[1]).is_eq())
    {
        return Err(invariant(
            "one operation emitted a duplicate open-order quantity delta",
        ));
    }
    Ok(deltas)
}

fn delta(
    fact: &ContinuousExecutionFact,
    account: AccountId,
    stock: StockCode,
    order_id: OrderId,
    value: i8,
    kind: ContinuousOpenOrderDeltaKind,
) -> ContinuousOpenOrderDelta {
    ContinuousOpenOrderDelta {
        candidate_key: fact.candidate_key.clone(),
        sealed_index: fact.sealed_index,
        account,
        stock,
        order_id,
        delta: value,
        kind,
    }
}

fn validate_new_operation_identities(
    coordinator: &IncrementalContinuousStockCoordinator,
    operations: &[P3ValidatedOperation],
) -> Result<(), StepFatal> {
    let mut last_candidate = coordinator.last_candidate_key.as_ref();
    let mut last_sealed = coordinator.last_sealed_index;
    let mut candidates = BTreeSet::new();
    let mut sealed = BTreeSet::new();
    for operation in operations {
        if last_candidate.is_some_and(|previous| previous >= operation.candidate_key()) {
            return Err(invariant(
                "incremental P4 candidate keys are not globally increasing",
            ));
        }
        if last_sealed.is_some_and(|previous| previous >= operation.sealed_index()) {
            return Err(invariant(
                "incremental P4 sealed identities are not globally increasing",
            ));
        }
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
        last_candidate = Some(operation.candidate_key());
        last_sealed = Some(operation.sealed_index());
    }
    Ok(())
}

fn canonicalize_round(
    facts: &mut [ContinuousExecutionFact],
    receipts: &mut [EnvelopeReceipt],
    trades: &mut [ContinuousTradeFact],
    deltas: &mut [ContinuousOpenOrderDelta],
) -> Result<(), StepFatal> {
    facts.sort_by(|left, right| {
        (left.sealed_index, &left.candidate_key).cmp(&(right.sealed_index, &right.candidate_key))
    });
    receipts.sort_by(|left, right| left.local_key.cmp(&right.local_key));
    trades.sort_by(|left, right| {
        (
            left.triggering_sealed_index,
            &left.stock,
            left.stock_local_trade_event_index,
        )
            .cmp(&(
                right.triggering_sealed_index,
                &right.stock,
                right.stock_local_trade_event_index,
            ))
    });
    deltas.sort_by(delta_cmp_key);

    if facts
        .windows(2)
        .any(|pair| pair[0].sealed_index == pair[1].sealed_index)
    {
        return Err(invariant(
            "incremental P4 round contains duplicate typed fact identities",
        ));
    }
    if receipts
        .windows(2)
        .any(|pair| pair[0].local_key == pair[1].local_key)
    {
        return Err(invariant(
            "incremental P4 round contains duplicate receipt identities",
        ));
    }
    Ok(())
}

fn delta_cmp_key(
    left: &ContinuousOpenOrderDelta,
    right: &ContinuousOpenOrderDelta,
) -> std::cmp::Ordering {
    (
        left.sealed_index,
        left.account,
        &left.stock,
        left.order_id,
        left.kind,
    )
        .cmp(&(
            right.sealed_index,
            right.account,
            &right.stock,
            right.order_id,
            right.kind,
        ))
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
