//! B2 call-auction and trading-day-boundary transaction.
//!
//! The stock worker drains its sealed operation stream before running the
//! indicative/final auction tail.  Closing-auction completion and DayEnd then
//! share one stock-local receipt batch.  The session adapter executes P5, P6,
//! event collection, and all boundary state changes on a private candidate;
//! the caller-provided session is replaced only after every fallible step has
//! succeeded.

use super::super::{
    Envelope, EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, EventStableKey, P2CandidateBatch,
    P2CandidateKey, P3PlaceKind, P3ValidatedOperation, P3ValidationOutput, ReceiptKind, StepFatal,
    p5_receipts::apply_session_receipt_transaction,
    p6_transaction::{P6TransactionError, P6TransactionOutput, apply_session_p6_transaction},
    p7_events::{OwnedEventFact, collect_events},
    p7_producers::adapt_p3_rejection_facts,
    stock_auction_adapter::{AuctionStockInput, prepare_incremental_auction_inputs},
};
use super::{
    AuctionCancelRejection, AuctionMatch, AuctionOperation, AuctionOperationFact, AuctionOrder,
    auction_indicative, complete_stock_auction, day_end_release_receipt, reject_receipt,
};
use crate::plans::PlanEvent;
use crate::session::{PendingPlanEvent, RetailOrderDiagnosticEvent};
use crate::{
    AuctionOrderSnap, Event, GameSession, Market, MarketError, Money, Order, OrderId,
    RejectionReason, StockCode, TradingPhase,
};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, thiserror::Error)]
pub(in crate::session::pipeline) enum B2AuctionDayEndError {
    #[error("B2 auction/day-end precondition failed: {0}")]
    Precondition(#[source] StepFatal),
    #[error("B2 auction adapter failed: {0}")]
    Adapter(#[source] StepFatal),
    #[error("B2 stock worker for {code:?} failed: {source}")]
    Worker {
        code: StockCode,
        #[source]
        source: StepFatal,
    },
    #[error("B2 receipt transaction failed: {0}")]
    P5(#[source] StepFatal),
    #[error("B2 settlement transaction failed: {0}")]
    P6(#[source] P6TransactionError),
    #[error("B2 order lifecycle projection failed: {0}")]
    Lifecycle(#[source] StepFatal),
    #[error("B2 event collection failed: {0}")]
    P7(#[source] StepFatal),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::session::pipeline) struct B2FinalizerAudit {
    pub(in crate::session::pipeline) auction_tail_passes: u8,
    pub(in crate::session::pipeline) auction_completion_passes: u8,
    pub(in crate::session::pipeline) day_end_passes: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct DayEndCancellationFact {
    key: EnvelopeKey,
    remaining_qty: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub(in crate::session::pipeline) enum AuctionLifecycleFact {
    Accepted {
        candidate_key: P2CandidateKey,
        sealed_index: u64,
        account: crate::AccountId,
        code: StockCode,
        order_id: OrderId,
        side: crate::Side,
        qty: u32,
    },
    Canceled {
        candidate_key: P2CandidateKey,
        sealed_index: u64,
        account: crate::AccountId,
        code: StockCode,
        order_id: OrderId,
        remaining_qty: u32,
    },
    Rejected {
        candidate_key: P2CandidateKey,
        sealed_index: u64,
        account: crate::AccountId,
        code: StockCode,
        order_id: Option<OrderId>,
        reason: RejectionReason,
    },
}

impl AuctionLifecycleFact {
    const fn sealed_index(&self) -> u64 {
        match self {
            Self::Accepted { sealed_index, .. }
            | Self::Canceled { sealed_index, .. }
            | Self::Rejected { sealed_index, .. } => *sealed_index,
        }
    }

    fn candidate_key(&self) -> &P2CandidateKey {
        match self {
            Self::Accepted { candidate_key, .. }
            | Self::Canceled { candidate_key, .. }
            | Self::Rejected { candidate_key, .. } => candidate_key,
        }
    }
}

/// Detached stock-local result.  Main can merge these containers with other
/// P4 producers before its single P5/P6 pass; no session authority is hidden in
/// this value.
pub(in crate::session::pipeline) struct B2AuctionStockOutput {
    pub(in crate::session::pipeline) code: StockCode,
    pub(in crate::session::pipeline) market: Market,
    pub(in crate::session::pipeline) auction_orders: Vec<AuctionOrderSnap>,
    pub(in crate::session::pipeline) created_envelopes: Vec<Envelope>,
    pub(in crate::session::pipeline) receipts: Vec<EnvelopeReceipt>,
    pub(in crate::session::pipeline) terminal_keys: Vec<EnvelopeKey>,
    pub(in crate::session::pipeline) event_facts: Vec<OwnedEventFact>,
    pub(in crate::session::pipeline) lifecycle_facts: Vec<AuctionLifecycleFact>,
    day_end_cancellations: Vec<DayEndCancellationFact>,
    pub(in crate::session::pipeline) matches: Vec<AuctionMatch>,
    pub(in crate::session::pipeline) clearing_price: Option<Money>,
    pub(in crate::session::pipeline) finalizer: B2FinalizerAudit,
}

pub(in crate::session::pipeline) struct B2AuctionDayEndOutput {
    pub(in crate::session::pipeline) events: Vec<Event>,
    pub(in crate::session::pipeline) receipts: Vec<EnvelopeReceipt>,
    pub(in crate::session::pipeline) p6: P6TransactionOutput,
    pub(in crate::session::pipeline) finalizer: B2FinalizerAudit,
}

/// One continuation-facing auction P4 result. Identity is carried explicitly; callers must not
/// infer it from the eventual P7 event order.
#[derive(Clone, Debug, PartialEq)]
pub(in crate::session::pipeline) struct AuctionExecutionFact {
    pub(in crate::session::pipeline) candidate_key: P2CandidateKey,
    pub(in crate::session::pipeline) sealed_index: u64,
    pub(in crate::session::pipeline) allocated_order_id: Option<OrderId>,
    pub(in crate::session::pipeline) outcome: AuctionLifecycleFact,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(in crate::session::pipeline) struct AuctionOpenOrderDelta {
    pub(in crate::session::pipeline) candidate_key: P2CandidateKey,
    pub(in crate::session::pipeline) sealed_index: u64,
    pub(in crate::session::pipeline) account: crate::AccountId,
    pub(in crate::session::pipeline) delta: i8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::session::pipeline) struct AuctionStockProjection {
    pub(in crate::session::pipeline) orders: Vec<AuctionOrderSnap>,
}

/// Only the operation facts produced by one `apply_round` call. AuctionTick, completion and
/// DayEnd facts cannot appear until the consuming `finish` boundary.
#[derive(Clone, Debug)]
pub(in crate::session::pipeline) struct AuctionExecutionRound {
    pub(in crate::session::pipeline) facts: Vec<AuctionExecutionFact>,
    pub(in crate::session::pipeline) receipts: Vec<EnvelopeReceipt>,
    pub(in crate::session::pipeline) projections: BTreeMap<StockCode, AuctionStockProjection>,
    pub(in crate::session::pipeline) open_order_deltas: Vec<AuctionOpenOrderDelta>,
}

pub(in crate::session::pipeline) struct IncrementalAuctionFinish {
    pub(in crate::session::pipeline) workers: BTreeMap<StockCode, B2AuctionStockOutput>,
    pub(in crate::session::pipeline) detached_event_facts: Vec<OwnedEventFact>,
    pub(in crate::session::pipeline) detached_lifecycle_facts: Vec<AuctionLifecycleFact>,
}

/// Tick-private per-stock auction state. It is initialized exactly once from post-P0 authority,
/// survives every P3/P4 continuation boundary, and exposes the tail only through consuming
/// `finish`.
#[derive(Clone, Debug)]
pub(in crate::session::pipeline) struct IncrementalAuctionStockCoordinator {
    stocks: BTreeMap<StockCode, AuctionStockShadow>,
    detached_event_facts: Vec<OwnedEventFact>,
    detached_lifecycle_facts: Vec<AuctionLifecycleFact>,
    seen_candidate_keys: BTreeSet<P2CandidateKey>,
    seen_sealed_indices: BTreeSet<u64>,
    last_candidate_key: Option<P2CandidateKey>,
    last_sealed_index: Option<u64>,
    applied_operation_count: usize,
}

#[derive(Clone, Debug)]
struct AuctionStockShadow {
    code: StockCode,
    market: Market,
    completion: super::AuctionCompletionInput,
    continuous_envelopes: Vec<Envelope>,
    ledger: EnvelopeLedger,
    created_envelopes: Vec<Envelope>,
    receipts: Vec<EnvelopeReceipt>,
    terminal_keys: Vec<EnvelopeKey>,
    event_facts: Vec<OwnedEventFact>,
    lifecycle_facts: Vec<AuctionLifecycleFact>,
}

struct AuctionStockRoundResult {
    code: StockCode,
    shadow: AuctionStockShadow,
    facts: Vec<AuctionExecutionFact>,
    receipts: Vec<EnvelopeReceipt>,
    open_order_deltas: Vec<AuctionOpenOrderDelta>,
}

impl IncrementalAuctionStockCoordinator {
    pub(in crate::session::pipeline) fn from_post_p0(
        inputs: Vec<AuctionStockInput>,
    ) -> Result<Self, StepFatal> {
        let mut stocks = BTreeMap::new();
        for input in inputs {
            if !input.operations.is_empty() {
                return Err(invariant(
                    "incremental auction initialization included sealed operations",
                ));
            }
            validate_worker_input(&input, false, false)?;
            let shadow = AuctionStockShadow::from_post_p0(input)?;
            if stocks.insert(shadow.code.clone(), shadow).is_some() {
                return Err(invariant(
                    "incremental auction initialized one stock shadow more than once",
                ));
            }
        }
        if stocks.is_empty() {
            return Err(invariant("incremental auction has no stock shadow"));
        }
        Ok(Self {
            stocks,
            detached_event_facts: Vec::new(),
            detached_lifecycle_facts: Vec::new(),
            seen_candidate_keys: BTreeSet::new(),
            seen_sealed_indices: BTreeSet::new(),
            last_candidate_key: None,
            last_sealed_index: None,
            applied_operation_count: 0,
        })
    }

    /// Applies one route round atomically. A typed error leaves the coordinator at the previous
    /// successful route boundary, including every per-stock queue and receipt outbox.
    pub(in crate::session::pipeline) fn apply_round(
        &mut self,
        operations: Vec<P3ValidatedOperation>,
    ) -> Result<AuctionExecutionRound, StepFatal> {
        let mut staged = self.clone();
        let round = staged.apply_round_in_place(operations)?;
        *self = staged;
        Ok(round)
    }

    fn apply_round_in_place(
        &mut self,
        operations: Vec<P3ValidatedOperation>,
    ) -> Result<AuctionExecutionRound, StepFatal> {
        validate_incremental_operation_identities(self, &operations)?;
        let mut grouped = BTreeMap::<StockCode, Vec<P3ValidatedOperation>>::new();
        let mut detached_facts = Vec::new();
        let mut detached_events = Vec::new();
        let mut detached_lifecycle = Vec::new();
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
                .ok_or_else(|| invariant("incremental auction operation count overflow"))?;
            let code = operation_code(&operation).clone();
            if self.stocks.contains_key(&code) {
                grouped.entry(code).or_default().push(operation);
                continue;
            }
            let P3ValidatedOperation::Cancel {
                account, order_id, ..
            } = operation
            else {
                return Err(invariant(
                    "P3 accepted an auction place for an unknown stock",
                ));
            };
            let lifecycle = AuctionLifecycleFact::Rejected {
                candidate_key: candidate_key.clone(),
                sealed_index,
                account,
                code: code.clone(),
                order_id: Some(order_id),
                reason: RejectionReason::UnknownStock,
            };
            detached_events.push(owned_event(
                Event::IntentRejected {
                    seq: 0,
                    account,
                    code,
                    reason: RejectionReason::UnknownStock,
                },
                sealed_index,
            ));
            detached_facts.push(AuctionExecutionFact {
                candidate_key,
                sealed_index,
                allocated_order_id: None,
                outcome: lifecycle.clone(),
            });
            detached_lifecycle.push(lifecycle);
            let _ = order_id;
        }

        let work = grouped
            .into_iter()
            .map(|(code, operations)| {
                let shadow = self.stocks.get(&code).cloned().ok_or_else(|| {
                    invariant("auction stock partition lost its initialized shadow")
                })?;
                Ok((code, shadow, operations))
            })
            .collect::<Result<Vec<_>, StepFatal>>()?;
        let results = work
            .into_par_iter()
            .map(|(code, shadow, operations)| apply_auction_stock_round(code, shadow, operations))
            .collect::<Vec<_>>();

        let mut facts = detached_facts;
        let mut receipts = Vec::new();
        let mut projections = BTreeMap::new();
        let mut open_order_deltas = Vec::new();
        for result in results {
            let result = result?;
            facts.extend(result.facts);
            receipts.extend(result.receipts);
            open_order_deltas.extend(result.open_order_deltas);
            projections.insert(
                result.code.clone(),
                AuctionStockProjection {
                    orders: result
                        .shadow
                        .completion
                        .state
                        .orders()
                        .iter()
                        .map(snapshot_order)
                        .collect(),
                },
            );
            self.stocks.insert(result.code, result.shadow);
        }
        self.detached_event_facts.extend(detached_events);
        self.detached_lifecycle_facts.extend(detached_lifecycle);
        canonicalize_auction_round(&mut facts, &mut receipts, &mut open_order_deltas)?;
        Ok(AuctionExecutionRound {
            facts,
            receipts,
            projections,
            open_order_deltas,
        })
    }

    pub(in crate::session::pipeline) fn finish(
        self,
        tick_after: u64,
        finish_auction: bool,
        finish_day: bool,
    ) -> Result<IncrementalAuctionFinish, StepFatal> {
        let mut workers = BTreeMap::new();
        let mut fact_count = self.detached_lifecycle_facts.len();
        for (code, shadow) in self.stocks {
            fact_count = fact_count
                .checked_add(shadow.lifecycle_facts.len())
                .ok_or_else(|| invariant("incremental auction fact count overflow"))?;
            let worker = finish_auction_stock(shadow, tick_after, finish_auction, finish_day)?;
            if workers.insert(code, worker).is_some() {
                return Err(invariant("incremental auction finish duplicated a stock"));
            }
        }
        if fact_count != self.applied_operation_count {
            return Err(invariant(
                "incremental auction did not produce exactly one fact per operation",
            ));
        }
        Ok(IncrementalAuctionFinish {
            workers,
            detached_event_facts: self.detached_event_facts,
            detached_lifecycle_facts: self.detached_lifecycle_facts,
        })
    }
}

impl AuctionStockShadow {
    fn from_post_p0(input: AuctionStockInput) -> Result<Self, StepFatal> {
        let existing = input
            .completion
            .state
            .orders()
            .iter()
            .map(|order| order.envelope.clone())
            .chain(input.continuous_envelopes.iter().cloned());
        let ledger = EnvelopeLedger::new(0, existing)?;
        Ok(Self {
            code: input.code,
            market: input.market,
            completion: input.completion,
            continuous_envelopes: input.continuous_envelopes,
            ledger,
            created_envelopes: Vec::new(),
            receipts: Vec::new(),
            terminal_keys: Vec::new(),
            event_facts: Vec::new(),
            lifecycle_facts: Vec::new(),
        })
    }
}

fn apply_auction_stock_round(
    code: StockCode,
    mut shadow: AuctionStockShadow,
    operations: Vec<P3ValidatedOperation>,
) -> Result<AuctionStockRoundResult, StepFatal> {
    let receipt_start = shadow.receipts.len();
    let fact_start = shadow.lifecycle_facts.len();
    let mut facts = Vec::with_capacity(operations.len());
    let mut open_order_deltas = Vec::new();
    for operation in operations {
        let fact = apply_auction_operation(&mut shadow, operation)?;
        match &fact.outcome {
            AuctionLifecycleFact::Accepted {
                candidate_key,
                sealed_index,
                account,
                ..
            } => open_order_deltas.push(AuctionOpenOrderDelta {
                candidate_key: candidate_key.clone(),
                sealed_index: *sealed_index,
                account: *account,
                delta: 1,
            }),
            AuctionLifecycleFact::Canceled {
                candidate_key,
                sealed_index,
                account,
                ..
            } => open_order_deltas.push(AuctionOpenOrderDelta {
                candidate_key: candidate_key.clone(),
                sealed_index: *sealed_index,
                account: *account,
                delta: -1,
            }),
            AuctionLifecycleFact::Rejected { .. } => {}
        }
        facts.push(fact);
    }
    if shadow.lifecycle_facts.len().saturating_sub(fact_start) != facts.len() {
        return Err(invariant(
            "auction round lifecycle fact count disagrees with operation count",
        ));
    }
    let receipts = shadow.receipts[receipt_start..].to_vec();
    Ok(AuctionStockRoundResult {
        code,
        shadow,
        facts,
        receipts,
        open_order_deltas,
    })
}

fn finish_auction_stock(
    mut shadow: AuctionStockShadow,
    tick_after: u64,
    finish_auction: bool,
    finish_day: bool,
) -> Result<B2AuctionStockOutput, StepFatal> {
    let input = AuctionStockInput {
        code: shadow.code.clone(),
        market: shadow.market,
        completion: shadow.completion,
        continuous_envelopes: shadow.continuous_envelopes,
        operations: Vec::new(),
    };
    let mut tail = process_b2_auction_stock(input, tick_after, finish_auction, finish_day)?;
    shadow.created_envelopes.append(&mut tail.created_envelopes);
    shadow.receipts.append(&mut tail.receipts);
    shadow.terminal_keys.append(&mut tail.terminal_keys);
    shadow.event_facts.append(&mut tail.event_facts);
    shadow.lifecycle_facts.append(&mut tail.lifecycle_facts);
    shadow
        .created_envelopes
        .sort_by(|left, right| left.key().cmp(right.key()));
    shadow.terminal_keys.sort();
    Ok(B2AuctionStockOutput {
        code: tail.code,
        market: tail.market,
        auction_orders: tail.auction_orders,
        created_envelopes: shadow.created_envelopes,
        receipts: shadow.receipts,
        terminal_keys: shadow.terminal_keys,
        event_facts: shadow.event_facts,
        lifecycle_facts: shadow.lifecycle_facts,
        day_end_cancellations: tail.day_end_cancellations,
        matches: tail.matches,
        clearing_price: tail.clearing_price,
        finalizer: tail.finalizer,
    })
}

fn apply_auction_operation(
    shadow: &mut AuctionStockShadow,
    operation: P3ValidatedOperation,
) -> Result<AuctionExecutionFact, StepFatal> {
    let candidate_key = operation.candidate_key().clone();
    let sealed_index = operation.sealed_index();
    let (allocated_order_id, lifecycle, event) = match operation {
        P3ValidatedOperation::Place(draft) => {
            let envelope = draft.materialize_envelope();
            envelope.validate()?;
            shadow.ledger.insert_created([envelope.clone()])?;
            shadow.created_envelopes.push(envelope.clone());
            let order = AuctionOrder {
                envelope,
                arrival_seq: draft.order_id().0,
            };
            if let Some(reason) = place_rejection(&shadow.market, &shadow.completion, &draft)? {
                let receipt = reject_receipt(&order, sealed_index)?;
                apply_local(
                    &mut shadow.ledger,
                    std::slice::from_ref(&receipt),
                    std::slice::from_ref(draft.key()),
                )?;
                shadow.receipts.push(receipt);
                shadow.terminal_keys.push(draft.key().clone());
                (
                    Some(draft.order_id()),
                    AuctionLifecycleFact::Rejected {
                        candidate_key: candidate_key.clone(),
                        sealed_index,
                        account: draft.owner(),
                        code: draft.code().clone(),
                        order_id: Some(draft.order_id()),
                        reason: reason.clone(),
                    },
                    Event::IntentRejected {
                        seq: 0,
                        account: draft.owner(),
                        code: draft.code().clone(),
                        reason,
                    },
                )
            } else {
                let output = shadow
                    .completion
                    .state
                    .apply_operation(shadow.completion.phase, AuctionOperation::Place(order))?;
                if output.receipt.is_some() || output.terminal_key.is_some() {
                    return Err(invariant(
                        "accepted auction placement produced a terminal transition",
                    ));
                }
                (
                    Some(draft.order_id()),
                    AuctionLifecycleFact::Accepted {
                        candidate_key: candidate_key.clone(),
                        sealed_index,
                        account: draft.owner(),
                        code: draft.code().clone(),
                        order_id: draft.order_id(),
                        side: draft.side(),
                        qty: draft.qty(),
                    },
                    Event::OrderAccepted {
                        seq: 0,
                        account: draft.owner(),
                        code: draft.code().clone(),
                        id: draft.order_id(),
                        side: draft.side(),
                        price: draft.limit(),
                        remaining_qty: draft.qty(),
                    },
                )
            }
        }
        P3ValidatedOperation::Cancel {
            account,
            code,
            order_id,
            ..
        } => {
            let output = shadow.completion.state.apply_operation(
                shadow.completion.phase,
                AuctionOperation::Cancel {
                    sealed_index,
                    account,
                    order_id,
                },
            )?;
            if let Some(receipt) = output.receipt {
                let terminal = output.terminal_key.ok_or_else(|| {
                    invariant("auction cancellation receipt has no terminal envelope")
                })?;
                apply_local(
                    &mut shadow.ledger,
                    std::slice::from_ref(&receipt),
                    std::slice::from_ref(&terminal),
                )?;
                shadow.receipts.push(receipt);
                shadow.terminal_keys.push(terminal);
            } else if output.terminal_key.is_some() {
                return Err(invariant(
                    "rejected auction cancellation exposed a terminal envelope",
                ));
            }
            let (lifecycle, event) = match output.fact {
                AuctionOperationFact::Canceled {
                    account,
                    order_id,
                    remaining_qty,
                } => (
                    AuctionLifecycleFact::Canceled {
                        candidate_key: candidate_key.clone(),
                        sealed_index,
                        account,
                        code: code.clone(),
                        order_id,
                        remaining_qty,
                    },
                    Event::OrderCanceled {
                        seq: 0,
                        account,
                        code,
                        id: order_id,
                        remaining_qty,
                    },
                ),
                AuctionOperationFact::Rejected {
                    account, reason, ..
                } => {
                    let reason = cancel_rejection(reason);
                    (
                        AuctionLifecycleFact::Rejected {
                            candidate_key: candidate_key.clone(),
                            sealed_index,
                            account,
                            code: code.clone(),
                            order_id: Some(order_id),
                            reason: reason.clone(),
                        },
                        Event::IntentRejected {
                            seq: 0,
                            account,
                            code,
                            reason,
                        },
                    )
                }
                AuctionOperationFact::Placed { .. } => {
                    return Err(invariant(
                        "auction cancel operation unexpectedly produced a placement fact",
                    ));
                }
            };
            (None, lifecycle, event)
        }
    };
    shadow.event_facts.push(owned_event(event, sealed_index));
    shadow.lifecycle_facts.push(lifecycle.clone());
    Ok(AuctionExecutionFact {
        candidate_key,
        sealed_index,
        allocated_order_id,
        outcome: lifecycle,
    })
}

fn validate_incremental_operation_identities(
    coordinator: &IncrementalAuctionStockCoordinator,
    operations: &[P3ValidatedOperation],
) -> Result<(), StepFatal> {
    if operations
        .windows(2)
        .any(|pair| pair[0].sealed_index() >= pair[1].sealed_index())
    {
        return Err(invariant(
            "incremental auction round is not in strict sealed order",
        ));
    }
    for operation in operations {
        if coordinator
            .last_candidate_key
            .as_ref()
            .is_some_and(|last| operation.candidate_key() <= last)
            || coordinator
                .last_sealed_index
                .is_some_and(|last| operation.sealed_index() <= last)
            || coordinator
                .seen_candidate_keys
                .contains(operation.candidate_key())
            || coordinator
                .seen_sealed_indices
                .contains(&operation.sealed_index())
        {
            return Err(invariant(
                "incremental auction operation identity was replayed or regressed",
            ));
        }
        let order_id = match operation {
            P3ValidatedOperation::Place(draft) => draft.order_id(),
            P3ValidatedOperation::Cancel { order_id, .. } => *order_id,
        };
        if matches!(operation, P3ValidatedOperation::Place(_))
            && order_id.0 >= crate::orderbook::js_safe_u64::MAX
        {
            return Err(invariant(
                "incremental auction place cannot preserve a serializable next order id",
            ));
        }
        if order_id.0 > crate::orderbook::js_safe_u64::MAX {
            return Err(invariant(
                "incremental auction operation order id exceeds serializable authority",
            ));
        }
    }
    Ok(())
}

fn canonicalize_auction_round(
    facts: &mut [AuctionExecutionFact],
    receipts: &mut [EnvelopeReceipt],
    deltas: &mut [AuctionOpenOrderDelta],
) -> Result<(), StepFatal> {
    facts.sort_by_key(|fact| fact.sealed_index);
    receipts.sort_by(|left, right| left.local_key.cmp(&right.local_key));
    deltas.sort_by(|left, right| {
        (left.sealed_index, left.account).cmp(&(right.sealed_index, right.account))
    });
    if facts
        .windows(2)
        .any(|pair| pair[0].sealed_index == pair[1].sealed_index)
        || receipts
            .windows(2)
            .any(|pair| pair[0].local_key == pair[1].local_key)
    {
        return Err(invariant(
            "auction round produced a duplicate operation or receipt identity",
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

/// Applies B2 to a prospective session atomically.
///
/// This is deliberately a candidate seam, not a `GameSession::step` cutover.
/// Main may call it on its full-tick shadow after the operation stream has
/// drained, or consume [`process_b2_auction_stock`] directly when composing a
/// joint B1/B2 P5 batch.
pub(super) fn apply_session_b2_auction_day_end_transaction(
    session: &mut GameSession,
    candidates: &P2CandidateBatch,
    validation: &P3ValidationOutput,
) -> Result<B2AuctionDayEndOutput, B2AuctionDayEndError> {
    let mut candidate = session
        .clone_for_tick_shadow()
        .map_err(B2AuctionDayEndError::Precondition)?;
    let output = apply_candidate(&mut candidate, candidates, validation)?;
    session.commit_tick_shadow(candidate);
    Ok(output)
}

fn apply_candidate(
    session: &mut GameSession,
    candidates: &P2CandidateBatch,
    validation: &P3ValidationOutput,
) -> Result<B2AuctionDayEndOutput, B2AuctionDayEndError> {
    validate_order_cursor(session, validation)?;
    let phase = session.phase();
    if !matches!(
        phase,
        TradingPhase::CallAuction | TradingPhase::ClosingAuction
    ) {
        return Err(B2AuctionDayEndError::Precondition(invariant(
            "B2 auction transaction requires an opening or closing auction phase",
        )));
    }
    let tick_after = session
        .tick
        .checked_add(1)
        .ok_or_else(|| B2AuctionDayEndError::Precondition(invariant("tick overflow")))?;
    let finish_auction = match phase {
        TradingPhase::CallAuction => {
            tick_after % session.setup.ticks_per_day == session.auction_entry_ticks()
        }
        TradingPhase::ClosingAuction => tick_after.is_multiple_of(session.setup.ticks_per_day),
        TradingPhase::PreOpen | TradingPhase::Continuous => false,
    };
    let finish_day =
        session.setup.ticks_per_day > 0 && tick_after.is_multiple_of(session.setup.ticks_per_day);
    if finish_day && !finish_auction {
        return Err(B2AuctionDayEndError::Precondition(invariant(
            "auction day boundary did not coincide with auction completion",
        )));
    }

    let facts = adapt_p3_rejection_facts(candidates, validation.results())
        .map_err(B2AuctionDayEndError::Adapter)?;
    let coordinator_lifecycle_facts =
        rejection_lifecycle_facts(candidates, validation).map_err(B2AuctionDayEndError::Adapter)?;
    let inputs =
        prepare_incremental_auction_inputs(session).map_err(B2AuctionDayEndError::Adapter)?;
    let mut coordinator = IncrementalAuctionStockCoordinator::from_post_p0(inputs)
        .map_err(B2AuctionDayEndError::Adapter)?;
    coordinator
        .apply_round(validation.operations().to_vec())
        .map_err(|source| B2AuctionDayEndError::Worker {
            code: StockCode("<incremental>".to_owned()),
            source,
        })?;
    let finish = coordinator
        .finish(tick_after, finish_auction, finish_day)
        .map_err(|source| B2AuctionDayEndError::Worker {
            code: StockCode("<incremental>".to_owned()),
            source,
        })?;
    apply_finished_candidate(
        session,
        validation,
        finish,
        facts,
        coordinator_lifecycle_facts,
        tick_after,
        finish_auction,
        finish_day,
        None,
    )
}

pub(in crate::session::pipeline) fn apply_incremental_auction_finish(
    session: &mut GameSession,
    candidates: &P2CandidateBatch,
    validation: &P3ValidationOutput,
    finish: IncrementalAuctionFinish,
    mut preceding_facts: Vec<OwnedEventFact>,
    consumed: &super::super::adaptive_plan_chain::PlanChainFactConsumption,
) -> Result<B2AuctionDayEndOutput, B2AuctionDayEndError> {
    validate_order_cursor(session, validation)?;
    let (tick_after, finish_auction, finish_day) = auction_tail_boundaries(session)?;
    preceding_facts.extend(
        adapt_p3_rejection_facts(candidates, validation.results())
            .map_err(B2AuctionDayEndError::Adapter)?,
    );
    let coordinator_lifecycle_facts =
        rejection_lifecycle_facts(candidates, validation).map_err(B2AuctionDayEndError::Adapter)?;
    apply_finished_candidate(
        session,
        validation,
        finish,
        preceding_facts,
        coordinator_lifecycle_facts,
        tick_after,
        finish_auction,
        finish_day,
        Some(consumed),
    )
}

pub(in crate::session::pipeline) fn finish_incremental_auction_coordinator(
    session: &GameSession,
    coordinator: IncrementalAuctionStockCoordinator,
) -> Result<IncrementalAuctionFinish, B2AuctionDayEndError> {
    let (tick_after, finish_auction, finish_day) = auction_tail_boundaries(session)?;
    coordinator
        .finish(tick_after, finish_auction, finish_day)
        .map_err(|source| B2AuctionDayEndError::Worker {
            code: StockCode("<incremental>".to_owned()),
            source,
        })
}

fn apply_finished_candidate(
    session: &mut GameSession,
    validation: &P3ValidationOutput,
    finish: IncrementalAuctionFinish,
    mut facts: Vec<OwnedEventFact>,
    mut coordinator_lifecycle_facts: Vec<AuctionLifecycleFact>,
    tick_after: u64,
    finish_auction: bool,
    finish_day: bool,
    consumed: Option<&super::super::adaptive_plan_chain::PlanChainFactConsumption>,
) -> Result<B2AuctionDayEndOutput, B2AuctionDayEndError> {
    let day_end_event_base = u64::try_from(validation.results().len()).map_err(|_| {
        B2AuctionDayEndError::Precondition(invariant(
            "P3 result count exceeds the event identity domain",
        ))
    })?;
    facts.extend(finish.detached_event_facts);
    coordinator_lifecycle_facts.extend(finish.detached_lifecycle_facts);
    let mut workers = finish.workers;
    let finalizer = B2FinalizerAudit {
        auction_tail_passes: 1,
        auction_completion_passes: u8::from(finish_auction),
        day_end_passes: u8::from(finish_day),
    };
    validate_worker_finalizers(workers.values(), finalizer)?;

    let mut day_end_cancellations = workers
        .values()
        .flat_map(|worker| worker.day_end_cancellations.iter().cloned())
        .collect::<Vec<_>>();
    day_end_cancellations.sort_by(|left, right| left.key.cmp(&right.key));
    for (ordinal, cancellation) in day_end_cancellations.into_iter().enumerate() {
        let ordinal = u64::try_from(ordinal).map_err(|_| {
            B2AuctionDayEndError::P7(invariant("DayEnd cancellation event ordinal exceeds u64"))
        })?;
        let local_index = day_end_event_base.checked_add(ordinal).ok_or_else(|| {
            B2AuctionDayEndError::P7(invariant("DayEnd cancellation event index overflow"))
        })?;
        facts.push(owned_event(
            Event::OrderCanceled {
                seq: 0,
                account: cancellation.key.account,
                code: cancellation.key.stock,
                id: cancellation.key.order,
                remaining_qty: cancellation.remaining_qty,
            },
            local_index,
        ));
    }

    let mut created = Vec::new();
    let mut receipt_batches = Vec::new();
    let mut terminals = Vec::new();
    for worker in workers.values_mut() {
        created.append(&mut worker.created_envelopes);
        receipt_batches.push(std::mem::take(&mut worker.receipts));
        terminals.append(&mut worker.terminal_keys);
        facts.append(&mut worker.event_facts);
    }
    let receipts = apply_session_receipt_transaction(session, created, receipt_batches, terminals)
        .map_err(B2AuctionDayEndError::P5)?;
    let p6 = apply_session_p6_transaction(session, &receipts).map_err(B2AuctionDayEndError::P6)?;
    apply_auction_lifecycle_projection(
        session,
        workers.values(),
        &coordinator_lifecycle_facts,
        &receipts,
        finish_day,
        consumed,
    )
    .map_err(B2AuctionDayEndError::Lifecycle)?;
    if !finish_day {
        let mut plans = std::mem::take(&mut session.plans);
        let synchronized = session.synchronize_plan_execution(&mut plans);
        session.plans = plans;
        synchronized.map_err(|error| {
            B2AuctionDayEndError::Lifecycle(lifecycle_invariant(&format!(
                "final auction plan synchronization failed: {error}"
            )))
        })?;
    }

    for (code, worker) in &workers {
        session.markets.insert(code.clone(), worker.market.clone());
        if finish_auction {
            if worker.matches.is_empty() {
                let last = worker.market.last_price();
                session.update_active_daily_candle(code, last, 0);
            } else {
                for matched in &worker.matches {
                    session.update_active_daily_candle(code, matched.price, u64::from(matched.qty));
                }
            }
        }
    }
    session.auction_orders = workers
        .iter()
        .filter_map(|(code, worker)| {
            (!worker.auction_orders.is_empty())
                .then_some((code.clone(), worker.auction_orders.clone()))
        })
        .collect();
    session.auction_order_counts.clear();
    for order in session.auction_orders.values().flatten() {
        let count = session.auction_order_counts.entry(order.owner).or_default();
        *count = count.checked_add(1).ok_or_else(|| {
            B2AuctionDayEndError::Precondition(invariant(
                "auction order count overflow after B2 worker collection",
            ))
        })?;
    }
    session.next_order_id = validation.next_order_id_after();
    session.tick = tick_after;

    if finish_day {
        let mut boundary_facts = finalize_trading_day(session)?;
        facts.append(&mut boundary_facts);
    }
    let collected = collect_events(facts, session.seq).map_err(B2AuctionDayEndError::P7)?;
    session.seq = collected.next_seq;

    Ok(B2AuctionDayEndOutput {
        events: collected.events,
        receipts,
        p6,
        finalizer,
    })
}

fn auction_tail_boundaries(
    session: &GameSession,
) -> Result<(u64, bool, bool), B2AuctionDayEndError> {
    let phase = session.phase();
    if !matches!(
        phase,
        TradingPhase::CallAuction | TradingPhase::ClosingAuction
    ) {
        return Err(B2AuctionDayEndError::Precondition(invariant(
            "B2 auction transaction requires an opening or closing auction phase",
        )));
    }
    let tick_after = session
        .tick
        .checked_add(1)
        .ok_or_else(|| B2AuctionDayEndError::Precondition(invariant("tick overflow")))?;
    let finish_auction = match phase {
        TradingPhase::CallAuction => {
            tick_after % session.setup.ticks_per_day == session.auction_entry_ticks()
        }
        TradingPhase::ClosingAuction => tick_after.is_multiple_of(session.setup.ticks_per_day),
        TradingPhase::PreOpen | TradingPhase::Continuous => false,
    };
    let finish_day =
        session.setup.ticks_per_day > 0 && tick_after.is_multiple_of(session.setup.ticks_per_day);
    if finish_day && !finish_auction {
        return Err(B2AuctionDayEndError::Precondition(invariant(
            "auction day boundary did not coincide with auction completion",
        )));
    }
    Ok((tick_after, finish_auction, finish_day))
}

/// Drains one stock's already-sealed auction operations, then executes its
/// indicative/completion/day-end tail exactly once.
pub(super) fn process_b2_auction_stock(
    mut input: AuctionStockInput,
    tick_after: u64,
    finish_auction: bool,
    finish_day: bool,
) -> Result<B2AuctionStockOutput, StepFatal> {
    validate_worker_input(&input, finish_auction, finish_day)?;
    let mut created_envelopes = input
        .operations
        .iter()
        .filter_map(|operation| match operation {
            P3ValidatedOperation::Place(draft) => Some(draft.materialize_envelope()),
            P3ValidatedOperation::Cancel { .. } => None,
        })
        .collect::<Vec<_>>();
    for envelope in &created_envelopes {
        envelope.validate()?;
    }
    let existing = input
        .completion
        .state
        .orders()
        .iter()
        .map(|order| order.envelope.clone())
        .chain(input.continuous_envelopes.iter().cloned())
        .chain(created_envelopes.iter().cloned());
    let mut local_ledger = EnvelopeLedger::new(0, existing)?;
    let mut receipts = Vec::new();
    let mut terminal_keys = Vec::new();
    let mut event_facts = Vec::new();
    let mut lifecycle_facts = Vec::new();

    for operation in input.operations {
        match operation {
            P3ValidatedOperation::Place(draft) => {
                let order = AuctionOrder {
                    envelope: draft.materialize_envelope(),
                    arrival_seq: draft.order_id().0,
                };
                let rejection = place_rejection(&input.market, &input.completion, &draft)?;
                if let Some(reason) = rejection {
                    let receipt = reject_receipt(&order, draft.sealed_index())?;
                    apply_local(
                        &mut local_ledger,
                        std::slice::from_ref(&receipt),
                        std::slice::from_ref(draft.key()),
                    )?;
                    receipts.push(receipt);
                    terminal_keys.push(draft.key().clone());
                    event_facts.push(owned_event(
                        Event::IntentRejected {
                            seq: 0,
                            account: draft.owner(),
                            code: draft.code().clone(),
                            reason: reason.clone(),
                        },
                        draft.sealed_index(),
                    ));
                    lifecycle_facts.push(AuctionLifecycleFact::Rejected {
                        candidate_key: draft.candidate_key().clone(),
                        sealed_index: draft.sealed_index(),
                        account: draft.owner(),
                        code: draft.code().clone(),
                        order_id: Some(draft.order_id()),
                        reason,
                    });
                    continue;
                }
                let output = input
                    .completion
                    .state
                    .apply_operation(input.completion.phase, AuctionOperation::Place(order))?;
                if output.receipt.is_some() || output.terminal_key.is_some() {
                    return Err(invariant(
                        "accepted auction placement produced a terminal transition",
                    ));
                }
                event_facts.push(owned_event(
                    Event::OrderAccepted {
                        seq: 0,
                        account: draft.owner(),
                        code: draft.code().clone(),
                        id: draft.order_id(),
                        side: draft.side(),
                        price: draft.limit(),
                        remaining_qty: draft.qty(),
                    },
                    draft.sealed_index(),
                ));
                lifecycle_facts.push(AuctionLifecycleFact::Accepted {
                    candidate_key: draft.candidate_key().clone(),
                    sealed_index: draft.sealed_index(),
                    account: draft.owner(),
                    code: draft.code().clone(),
                    order_id: draft.order_id(),
                    side: draft.side(),
                    qty: draft.qty(),
                });
            }
            P3ValidatedOperation::Cancel {
                candidate_key,
                sealed_index,
                account,
                code,
                order_id,
                ..
            } => {
                let output = input.completion.state.apply_operation(
                    input.completion.phase,
                    AuctionOperation::Cancel {
                        sealed_index,
                        account,
                        order_id,
                    },
                )?;
                if let Some(receipt) = output.receipt {
                    let terminal = output.terminal_key.ok_or_else(|| {
                        invariant("auction cancellation receipt has no terminal envelope")
                    })?;
                    apply_local(
                        &mut local_ledger,
                        std::slice::from_ref(&receipt),
                        std::slice::from_ref(&terminal),
                    )?;
                    receipts.push(receipt);
                    terminal_keys.push(terminal);
                } else if output.terminal_key.is_some() {
                    return Err(invariant(
                        "rejected auction cancellation exposed a terminal envelope",
                    ));
                }
                if let AuctionOperationFact::Canceled {
                    account,
                    order_id,
                    remaining_qty,
                } = &output.fact
                {
                    lifecycle_facts.push(AuctionLifecycleFact::Canceled {
                        candidate_key: candidate_key.clone(),
                        sealed_index,
                        account: *account,
                        code: code.clone(),
                        order_id: *order_id,
                        remaining_qty: *remaining_qty,
                    });
                } else if let AuctionOperationFact::Rejected {
                    account, reason, ..
                } = &output.fact
                {
                    lifecycle_facts.push(AuctionLifecycleFact::Rejected {
                        candidate_key,
                        sealed_index,
                        account: *account,
                        code: code.clone(),
                        order_id: Some(order_id),
                        reason: cancel_rejection(*reason),
                    });
                }
                event_facts.push(operation_fact_event(output.fact, code, sealed_index)?);
            }
        }
    }

    let (indicative, imbalance) = auction_indicative(
        &input.completion.state,
        input.completion.previous_close,
        input.completion.exchange,
        input.completion.price_tick,
    )?;
    event_facts.push(owned_event(
        Event::AuctionTick {
            seq: 0,
            tick: tick_after,
            phase: trading_phase(input.completion.phase),
            code: input.code.clone(),
            indicative_price: indicative.map(|selection| selection.price),
            matched_volume: indicative.map_or(0, |selection| selection.volume),
            imbalance,
        },
        0,
    ));

    let mut market = input.market;
    let mut auction_orders = Vec::new();
    let mut day_end_cancellations = Vec::new();
    let mut matches = Vec::new();
    let mut clearing_price = None;
    if finish_auction {
        let completion_phase = input.completion.phase;
        input.completion.day_end_envelopes = if finish_day {
            input
                .continuous_envelopes
                .iter()
                .map(|envelope| envelope.key().clone())
                .collect()
        } else {
            Vec::new()
        };
        let mut completion = complete_stock_auction(input.completion)?;
        clearing_price = completion.clearing.map(|selection| selection.price);
        if let Some(price) = clearing_price {
            market.set_last_price(price);
        }
        apply_local(
            &mut local_ledger,
            &completion.receipts,
            &completion.terminal_keys,
        )?;
        receipts.append(&mut completion.receipts);
        terminal_keys.append(&mut completion.terminal_keys);
        matches = completion.matches;

        if finish_day {
            let mut continuous_receipts = Vec::new();
            let mut continuous_terminals = Vec::new();
            for envelope in &input.continuous_envelopes {
                let source = completion
                    .day_end_source_indices
                    .get(envelope.key())
                    .copied()
                    .ok_or_else(|| invariant("continuous DayEnd envelope has no source index"))?;
                continuous_receipts.push(day_end_release_receipt(envelope, source)?);
                continuous_terminals.push(envelope.key().clone());
            }
            apply_local(
                &mut local_ledger,
                &continuous_receipts,
                &continuous_terminals,
            )?;
            receipts.extend(continuous_receipts);
            terminal_keys.extend(continuous_terminals);
            for receipt in receipts.iter().filter(|receipt| {
                receipt.kind == ReceiptKind::Release
                    && matches!(
                        receipt.local_key.source(),
                        super::super::ReceiptSource::DayEnd(_)
                    )
            }) {
                day_end_cancellations.push(DayEndCancellationFact {
                    key: receipt.envelope.clone(),
                    remaining_qty: receipt.qty_after,
                });
            }
            market.end_of_day();
        } else {
            market = stage_opening_remainders(market, completion.continuous_orders)?;
        }
        event_facts.extend(match_events(&input.code, &matches)?);
        event_facts.push(owned_event(
            Event::AuctionCompleted {
                seq: 0,
                tick: tick_after,
                phase: trading_phase(completion_phase),
                code: input.code.clone(),
                clearing_price,
                matched_volume: completion.matched_volume,
            },
            1,
        ));
    } else {
        auction_orders = input
            .completion
            .state
            .orders()
            .iter()
            .map(snapshot_order)
            .collect();
    }

    created_envelopes.sort_by(|left, right| left.key().cmp(right.key()));
    terminal_keys.sort();
    day_end_cancellations.sort_by(|left, right| left.key.cmp(&right.key));
    Ok(B2AuctionStockOutput {
        code: input.code,
        market,
        auction_orders,
        created_envelopes,
        receipts,
        terminal_keys,
        event_facts,
        lifecycle_facts,
        day_end_cancellations,
        matches,
        clearing_price,
        finalizer: B2FinalizerAudit {
            auction_tail_passes: 1,
            auction_completion_passes: u8::from(finish_auction),
            day_end_passes: u8::from(finish_day),
        },
    })
}

fn validate_worker_input(
    input: &AuctionStockInput,
    finish_auction: bool,
    finish_day: bool,
) -> Result<(), StepFatal> {
    if input.market.code() != &input.code {
        return Err(invariant("B2 stock input market identity mismatch"));
    }
    if finish_day && !finish_auction {
        return Err(invariant("B2 DayEnd requires auction completion"));
    }
    if finish_day && input.completion.phase != super::AuctionPhase::Closing {
        return Err(invariant("B2 DayEnd requires the closing auction"));
    }
    if input
        .operations
        .windows(2)
        .any(|pair| pair[0].sealed_index() >= pair[1].sealed_index())
    {
        return Err(invariant(
            "B2 stock operations are not in strict sealed order",
        ));
    }
    Ok(())
}

fn place_rejection(
    market: &Market,
    completion: &super::AuctionCompletionInput,
    draft: &super::super::EnvelopeDraft,
) -> Result<Option<RejectionReason>, StepFatal> {
    if draft.kind() == P3PlaceKind::Market {
        return Ok(Some(RejectionReason::AuctionLimitOrderRequired));
    }
    let tick = completion.price_tick.cents();
    if tick <= 0 || draft.limit() <= Money::ZERO || draft.limit().cents() % tick != 0 {
        return Err(invariant(
            "P3 accepted an auction limit price that is not a positive price tick",
        ));
    }
    let down = market
        .down_stop()
        .map_err(|error| invariant(&error.to_string()))?;
    let up = market
        .up_stop()
        .map_err(|error| invariant(&error.to_string()))?;
    Ok((draft.limit() < down || draft.limit() > up).then_some(RejectionReason::LimitExceeded))
}

fn stage_opening_remainders(
    mut market: Market,
    mut orders: Vec<AuctionOrder>,
) -> Result<Market, StepFatal> {
    orders.sort_by_key(|order| order.arrival_seq);
    for order in orders {
        let audit = order.envelope.audit();
        let original_qty = audit
            .filled_qty
            .checked_add(audit.remaining_qty)
            .ok_or_else(|| invariant("auction remainder original quantity overflow"))?;
        let result = market
            .place(Order {
                id: order.envelope.key().order,
                side: order.envelope.key().side,
                price: audit.limit,
                qty: audit.remaining_qty,
                original_qty,
                filled_qty: audit.filled_qty,
                filled_value: audit.filled_value,
                owner: order.envelope.key().account,
                seq: order.arrival_seq,
            })
            .map_err(market_error)?;
        if !result.trades.is_empty() || result.resting.is_none() {
            return Err(invariant(
                "auction remainder unexpectedly crossed during continuous-book rollover",
            ));
        }
    }
    Ok(market)
}

fn apply_local(
    ledger: &mut EnvelopeLedger,
    receipts: &[EnvelopeReceipt],
    terminal_keys: &[EnvelopeKey],
) -> Result<(), StepFatal> {
    let mut local = receipts.to_vec();
    ledger.apply(&mut local)?;
    ledger.remove_terminal(terminal_keys)
}

fn operation_fact_event(
    fact: AuctionOperationFact,
    code: StockCode,
    sealed_index: u64,
) -> Result<OwnedEventFact, StepFatal> {
    let event = match fact {
        AuctionOperationFact::Placed { .. } => {
            return Err(invariant(
                "auction cancel operation unexpectedly produced a placement fact",
            ));
        }
        AuctionOperationFact::Canceled {
            account,
            order_id,
            remaining_qty,
        } => Event::OrderCanceled {
            seq: 0,
            account,
            code,
            id: order_id,
            remaining_qty,
        },
        AuctionOperationFact::Rejected {
            account, reason, ..
        } => Event::IntentRejected {
            seq: 0,
            account,
            code,
            reason: cancel_rejection(reason),
        },
    };
    Ok(owned_event(event, sealed_index))
}

fn cancel_rejection(reason: AuctionCancelRejection) -> RejectionReason {
    match reason {
        AuctionCancelRejection::NotCancelable => RejectionReason::AuctionOrderNotCancelable,
        AuctionCancelRejection::OrderNotFound => RejectionReason::OrderNotFound,
        AuctionCancelRejection::NotOrderOwner => RejectionReason::NotOrderOwner,
        AuctionCancelRejection::SameTickEnvelope => RejectionReason::SameTickOrderNotCancelable,
    }
}

fn match_events(
    code: &StockCode,
    matches: &[AuctionMatch],
) -> Result<Vec<OwnedEventFact>, StepFatal> {
    matches
        .iter()
        .enumerate()
        .map(|(index, matched)| {
            let local_event_index = u64::try_from(index)
                .map_err(|_| invariant("auction match event index exceeds u64"))?;
            let (maker, taker) = if matched.buy.order.0 < matched.sell.order.0 {
                (matched.buy.account, matched.sell.account)
            } else {
                (matched.sell.account, matched.buy.account)
            };
            Ok(owned_event(
                Event::Trade {
                    seq: 0,
                    code: code.clone(),
                    price: matched.price,
                    qty: matched.qty,
                    maker,
                    taker,
                },
                local_event_index,
            ))
        })
        .collect()
}

fn snapshot_order(order: &AuctionOrder) -> AuctionOrderSnap {
    AuctionOrderSnap {
        owner: order.envelope.key().account,
        side: order.envelope.key().side,
        limit: order.envelope.audit().limit,
        qty: order.envelope.audit().remaining_qty,
        arrival_seq: order.arrival_seq,
    }
}

fn trading_phase(phase: super::AuctionPhase) -> TradingPhase {
    match phase {
        super::AuctionPhase::Opening { .. } => TradingPhase::CallAuction,
        super::AuctionPhase::Closing => TradingPhase::ClosingAuction,
    }
}

fn validate_worker_finalizers<'a>(
    workers: impl IntoIterator<Item = &'a B2AuctionStockOutput>,
    expected: B2FinalizerAudit,
) -> Result<(), B2AuctionDayEndError> {
    let mut count = 0_usize;
    for worker in workers {
        count = count.checked_add(1).ok_or_else(|| {
            B2AuctionDayEndError::Precondition(invariant("B2 worker count overflow"))
        })?;
        if worker.finalizer != expected {
            return Err(B2AuctionDayEndError::Precondition(invariant(
                "B2 stock workers disagree on finalizer pass counts",
            )));
        }
    }
    if count == 0 {
        return Err(B2AuctionDayEndError::Precondition(invariant(
            "B2 auction transaction has no stock worker",
        )));
    }
    Ok(())
}

fn apply_auction_lifecycle_projection<'a>(
    session: &mut GameSession,
    workers: impl IntoIterator<Item = &'a B2AuctionStockOutput>,
    coordinator_facts: &[AuctionLifecycleFact],
    receipts: &[EnvelopeReceipt],
    finish_day: bool,
    consumed: Option<&super::super::adaptive_plan_chain::PlanChainFactConsumption>,
) -> Result<(), StepFatal> {
    let workers = workers.into_iter().collect::<Vec<_>>();
    let mut operation_facts = coordinator_facts.to_vec();
    operation_facts.extend(
        workers
            .iter()
            .flat_map(|worker| worker.lifecycle_facts.iter().cloned()),
    );
    operation_facts.sort_by_key(AuctionLifecycleFact::sealed_index);
    if operation_facts
        .windows(2)
        .any(|pair| pair[0].sealed_index() == pair[1].sealed_index())
    {
        return Err(lifecycle_invariant(
            "auction lifecycle facts contain duplicate sealed identity",
        ));
    }

    let mut parents = session.parent_orders.clone();
    let mut pending = Vec::new();
    let mut retail_events = Vec::new();
    for fact in operation_facts {
        let already_projected = consumed.is_some_and(|consumed| {
            consumed
                .operations
                .contains(&(fact.candidate_key().clone(), fact.sealed_index()))
        });
        match fact {
            AuctionLifecycleFact::Accepted {
                account,
                code,
                order_id,
                side,
                qty,
                ..
            } => {
                if !already_projected {
                    if let Some(parent) = parents
                        .get_mut(&account)
                        .and_then(|plans| plans.get_mut(&code))
                        .filter(|parent| parent.side == side)
                    {
                        if parent.active_child_order_id.is_some()
                            || parent.active_child_remaining_qty.is_some()
                        {
                            return Err(lifecycle_invariant(
                                "linked parent accepted a second active auction child",
                            ));
                        }
                        parent.active_child_order_id = Some(order_id);
                        parent.active_child_remaining_qty = Some(qty);
                        if let Some(plan_id) = parent.linked_plan_id {
                            push_pending_plan_event(
                                session,
                                &mut pending,
                                PendingPlanEvent::Accepted {
                                    plan_id,
                                    order_id,
                                    trading_day: u64::from(session.day),
                                },
                            )?;
                        }
                    }
                }
                if session.retail_experience.contains_key(&account) {
                    retail_events.push(RetailOrderDiagnosticEvent::Submitted {
                        account,
                        code,
                        side,
                        order_id,
                        qty,
                    });
                }
            }
            AuctionLifecycleFact::Canceled {
                account,
                code,
                order_id,
                remaining_qty,
                ..
            } => {
                if !already_projected {
                    clear_parent_child(&mut parents, account, &code, order_id, remaining_qty)?;
                }
                if session.retail_experience.contains_key(&account) {
                    retail_events.push(RetailOrderDiagnosticEvent::Canceled {
                        account,
                        code,
                        order_id,
                        remaining_qty,
                    });
                }
            }
            AuctionLifecycleFact::Rejected {
                account,
                code,
                reason,
                ..
            } => {
                if session.retail_experience.contains_key(&account) {
                    retail_events.push(RetailOrderDiagnosticEvent::Rejected {
                        account,
                        code,
                        reason,
                    });
                }
            }
        }
    }

    for receipt in receipts
        .iter()
        .filter(|receipt| receipt.kind == ReceiptKind::Fill)
    {
        let qty = receipt
            .qty_before
            .checked_sub(receipt.qty_after)
            .ok_or_else(|| {
                lifecycle_invariant("auction fill receipt has a regressing quantity chain")
            })?;
        if qty == 0 {
            return Err(lifecycle_invariant(
                "auction lifecycle received a zero-quantity fill",
            ));
        }
        let key = &receipt.envelope;
        if let Some(parent) = parents
            .get_mut(&key.account)
            .and_then(|plans| plans.get_mut(&key.stock))
            .filter(|parent| {
                parent.side == key.side && parent.active_child_order_id == Some(key.order)
            })
        {
            let child_before = parent.active_child_remaining_qty.ok_or_else(|| {
                lifecycle_invariant("linked parent active child has no remaining quantity")
            })?;
            let child_after = child_before.checked_sub(qty).ok_or_else(|| {
                lifecycle_invariant("auction fill exceeds linked parent child quantity")
            })?;
            let filled_after = parent.filled_qty.checked_add(qty).ok_or_else(|| {
                lifecycle_invariant("linked parent auction fill quantity overflow")
            })?;
            if filled_after > parent.target_qty {
                return Err(lifecycle_invariant(
                    "linked parent auction fill exceeds its target quantity",
                ));
            }
            parent.filled_qty = filled_after;
            if child_after == 0 {
                parent.active_child_order_id = None;
                parent.active_child_remaining_qty = None;
            } else {
                parent.active_child_remaining_qty = Some(child_after);
            }
            if let Some(plan_id) = parent.linked_plan_id {
                push_pending_plan_event(
                    session,
                    &mut pending,
                    PendingPlanEvent::Filled {
                        plan_id,
                        order_id: key.order,
                        qty,
                        trading_day: u64::from(session.day),
                    },
                )?;
            }
        }
        if session.retail_experience.contains_key(&key.account) {
            retail_events.push(RetailOrderDiagnosticEvent::Filled {
                account: key.account,
                code: key.stock.clone(),
                side: key.side,
                order_id: key.order,
                qty,
            });
        }
    }

    let mut day_end_cancellations = workers
        .iter()
        .flat_map(|worker| worker.day_end_cancellations.iter().cloned())
        .collect::<Vec<_>>();
    day_end_cancellations.sort_by(|left, right| left.key.cmp(&right.key));
    for cancellation in day_end_cancellations {
        clear_parent_child(
            &mut parents,
            cancellation.key.account,
            &cancellation.key.stock,
            cancellation.key.order,
            cancellation.remaining_qty,
        )?;
        if session
            .retail_experience
            .contains_key(&cancellation.key.account)
        {
            retail_events.push(RetailOrderDiagnosticEvent::Canceled {
                account: cancellation.key.account,
                code: cancellation.key.stock,
                order_id: cancellation.key.order,
                remaining_qty: cancellation.remaining_qty,
            });
        }
    }

    for plans in parents.values_mut() {
        plans.retain(|_, parent| {
            parent.linked_plan_id.is_some() || parent.filled_qty < parent.target_qty
        });
    }
    parents.retain(|_, plans| !plans.is_empty());

    if finish_day {
        for parent in parents.values().flat_map(|plans| plans.values()) {
            if parent.filled_qty < parent.target_qty {
                if let Some(plan_id) = parent.linked_plan_id {
                    push_pending_plan_event(
                        session,
                        &mut pending,
                        PendingPlanEvent::DayEnded {
                            plan_id,
                            trading_day: u64::from(session.day),
                        },
                    )?;
                }
            }
        }
    }

    session.parent_orders = parents;
    session.pending_plan_events.extend(pending);
    session.last_retail_order_events.extend(retail_events);
    Ok(())
}

fn rejection_lifecycle_facts(
    candidates: &P2CandidateBatch,
    validation: &P3ValidationOutput,
) -> Result<Vec<AuctionLifecycleFact>, StepFatal> {
    let by_key = candidates
        .candidates()
        .iter()
        .map(|candidate| (candidate.key(), candidate))
        .collect::<BTreeMap<_, _>>();
    validation
        .results()
        .iter()
        .filter_map(|result| match result {
            super::super::P3CandidateResult::Rejected {
                key,
                sealed_index,
                reason,
            } => Some((key, *sealed_index, reason)),
            super::super::P3CandidateResult::Accepted { .. } => None,
        })
        .map(|(key, sealed_index, reason)| {
            let candidate = by_key
                .get(key)
                .ok_or_else(|| lifecycle_invariant("P3 rejection has no candidate payload"))?;
            let code = match candidate.intent() {
                crate::Intent::PlaceLimit { code, .. }
                | crate::Intent::PlaceMarket { code, .. }
                | crate::Intent::Cancel { code, .. } => code.clone(),
            };
            Ok(AuctionLifecycleFact::Rejected {
                candidate_key: key.clone(),
                sealed_index,
                account: candidate.owner(),
                code,
                order_id: match candidate.intent() {
                    crate::Intent::Cancel { id, .. } => Some(*id),
                    crate::Intent::PlaceLimit { .. } | crate::Intent::PlaceMarket { .. } => None,
                },
                reason: reason.clone(),
            })
        })
        .collect()
}

fn push_pending_plan_event(
    session: &GameSession,
    pending: &mut Vec<PendingPlanEvent>,
    event: PendingPlanEvent,
) -> Result<(), StepFatal> {
    let required = session
        .pending_plan_events
        .len()
        .checked_add(pending.len())
        .and_then(|used| used.checked_add(1))
        .ok_or_else(|| lifecycle_invariant("pending plan event count overflow"))?;
    if required > super::super::super::MAX_SAVED_PLAN_EVENTS {
        return Err(lifecycle_invariant(
            "pending plan event capacity cannot preserve auction lifecycle facts",
        ));
    }
    pending.push(event);
    Ok(())
}

fn clear_parent_child(
    parents: &mut BTreeMap<crate::AccountId, BTreeMap<StockCode, crate::session::ParentOrderPlan>>,
    account: crate::AccountId,
    code: &StockCode,
    order_id: OrderId,
    remaining_qty: u32,
) -> Result<(), StepFatal> {
    let Some(parent) = parents
        .get_mut(&account)
        .and_then(|plans| plans.get_mut(code))
    else {
        return Ok(());
    };
    if parent.active_child_order_id != Some(order_id) {
        return Ok(());
    }
    if parent.active_child_remaining_qty != Some(remaining_qty) {
        return Err(lifecycle_invariant(
            "auction cancellation disagrees with linked parent child quantity",
        ));
    }
    parent.active_child_order_id = None;
    parent.active_child_remaining_qty = None;
    Ok(())
}

fn finalize_trading_day(
    session: &mut GameSession,
) -> Result<Vec<OwnedEventFact>, B2AuctionDayEndError> {
    if session
        .markets
        .values()
        .any(|market| market.resting_order_count() != 0)
        || !session.auction_orders.is_empty()
        || session.envelope_ledger.iter().next().is_some()
    {
        return Err(B2AuctionDayEndError::Precondition(invariant(
            "DayEnd finalizer retained a live order or envelope",
        )));
    }
    if session.setup.t1_enabled {
        for account in session.accounts.values_mut() {
            account.unlock_t1_positions();
        }
    }
    checked_sweep_decision_chain_day_end(session).map_err(B2AuctionDayEndError::Lifecycle)?;
    session.parent_orders.clear();
    session.npc_order_lifecycles.clear();
    let closed_daily_candles = session.commit_active_daily_candles();
    session.day = session
        .day
        .checked_add(1)
        .ok_or_else(|| B2AuctionDayEndError::Precondition(invariant("trading day overflow")))?;
    for history in session.market_minute_closes.values_mut() {
        history.clear();
    }
    let facts = vec![owned_event(
        Event::DayBoundary {
            seq: 0,
            day: session.day,
            closed_daily_candles,
        },
        0,
    )];
    Ok(facts)
}

fn checked_sweep_decision_chain_day_end(session: &mut GameSession) -> Result<(), StepFatal> {
    let trading_day = u64::from(session.day);
    let mut plans = session.plans.clone();
    session
        .synchronize_plan_execution(&mut plans)
        .map_err(|error| lifecycle_invariant(&format!("plan synchronization failed: {error}")))?;
    let plan_ids = plans.plan_ids().collect::<Vec<_>>();
    for plan_id in plan_ids {
        let terminal = plans
            .plan(plan_id)
            .map_err(|error| lifecycle_invariant(&format!("plan lookup failed: {error}")))?
            .is_terminal();
        if !terminal {
            plans
                .apply(plan_id, PlanEvent::TradingDayEnded { trading_day })
                .map_err(|error| {
                    lifecycle_invariant(&format!(
                        "day-end plan sweep failed for {plan_id:?}: {error}"
                    ))
                })?;
        }
    }
    session.plans = plans;
    Ok(())
}

fn lifecycle_invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::b2_auction_day_end::lifecycle".to_owned(),
    }
}

fn validate_order_cursor(
    session: &GameSession,
    validation: &P3ValidationOutput,
) -> Result<(), B2AuctionDayEndError> {
    let draft_count = u64::try_from(validation.drafts().len()).map_err(|_| {
        B2AuctionDayEndError::Precondition(invariant(
            "P3 draft count exceeds the order identity domain",
        ))
    })?;
    let expected = session
        .next_order_id
        .checked_add(draft_count)
        .ok_or_else(|| {
            B2AuctionDayEndError::Precondition(invariant("B2 next order identity overflow"))
        })?;
    if validation.next_order_id_after() != expected {
        return Err(B2AuctionDayEndError::Precondition(invariant(
            "P3 next order cursor disagrees with the B2 session cursor",
        )));
    }
    for (ordinal, draft) in validation.drafts().iter().enumerate() {
        let ordinal = u64::try_from(ordinal).map_err(|_| {
            B2AuctionDayEndError::Precondition(invariant(
                "P3 draft ordinal exceeds the order identity domain",
            ))
        })?;
        let expected_id = session.next_order_id.checked_add(ordinal).ok_or_else(|| {
            B2AuctionDayEndError::Precondition(invariant("B2 draft order identity overflow"))
        })?;
        if draft.order_id() != OrderId(expected_id) {
            return Err(B2AuctionDayEndError::Precondition(invariant(
                "P3 draft identity disagrees with the B2 session cursor",
            )));
        }
    }
    Ok(())
}

fn owned_event(event: Event, local_event_index: u64) -> OwnedEventFact {
    OwnedEventFact {
        key: EventStableKey::for_event(&event, local_event_index),
        event,
    }
}

fn market_error(error: MarketError) -> StepFatal {
    invariant(&error.to_string())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::b2_auction_day_end".to_owned(),
    }
}
