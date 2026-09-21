use super::{
    transition::{BuyFillInput, FillTransition, SellFillInput},
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeOrigin, EnvelopeReceipt,
    FeeComponents, JournalRank, P3PlaceKind, P3ValidatedOperation, ReceiptDelta, ReceiptKind,
    ReceiptLocalKey, ReceiptSource, ReceiptTransition, ResVec, StepFatal,
};
use crate::{
    AccountId, GameConfig, Market, MarketError, Money, OrderError, OrderId, RejectionReason, Side,
    StockCode, Trade, TradingPhase,
};
use std::collections::{BTreeMap, BTreeSet};

#[path = "incremental_continuous_stock_shadow.rs"]
mod incremental_continuous_stock_shadow;

pub(super) use incremental_continuous_stock_shadow::{
    ContinuousExecutionRound, IncrementalContinuousStockCoordinator,
    IncrementalContinuousStockFinish,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ContinuousEnvelopeSnapshot {
    pub(super) envelope: Envelope,
    pub(super) audit: EnvelopeAudit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ContinuousCancelOperation {
    pub(super) sealed_index: u64,
    pub(super) account: AccountId,
    pub(super) code: StockCode,
    pub(super) order_id: OrderId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ContinuousCancelRejection {
    UnknownStock,
    OrderNotFound,
    NotOrderOwner,
    SameTickEnvelope,
    AuctionOrderNotCancelable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ContinuousCancelFact {
    Canceled {
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        side: Side,
        remaining_qty: u32,
    },
    Rejected {
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        reason: ContinuousCancelRejection,
    },
}

#[derive(Clone, Debug)]
pub(super) struct ContinuousCancelInput {
    pub(super) market: Market,
    pub(super) envelopes: Vec<ContinuousEnvelopeSnapshot>,
    pub(super) operation: ContinuousCancelOperation,
}

#[derive(Debug)]
pub(super) struct ContinuousCancelOutput {
    pub(super) market: Market,
    pub(super) receipt: Option<EnvelopeReceipt>,
    pub(super) terminal_key: Option<EnvelopeKey>,
    pub(super) fact: ContinuousCancelFact,
}

#[derive(Clone, Debug)]
pub(super) struct ContinuousStockInput {
    pub(super) phase: TradingPhase,
    pub(super) market: Market,
    pub(super) envelopes: Vec<ContinuousEnvelopeSnapshot>,
    pub(super) operations: Vec<P3ValidatedOperation>,
    pub(super) config: GameConfig,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ContinuousPlaceFact {
    Resting {
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        side: Side,
        price: Money,
        remaining_qty: u32,
    },
    Filled {
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        side: Side,
        filled_qty: u32,
    },
    Rejected {
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        reason: RejectionReason,
    },
}

/// The one typed P4 result associated with one P3-accepted operation.
///
/// This is the continuation-facing identity. Consumers must not reconstruct it from P7 events:
/// an immediately filled order deliberately has no `OrderAccepted` event, while a P4-rejected
/// place still owns its preallocated order ID.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ContinuousExecutionFact {
    pub(super) candidate_key: super::P2CandidateKey,
    pub(super) sealed_index: u64,
    pub(super) allocated_order_id: Option<OrderId>,
    pub(super) outcome: ContinuousExecutionOutcome,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ContinuousExecutionOutcome {
    Place {
        fact: ContinuousPlaceFact,
        original_qty: u32,
    },
    Cancel(ContinuousCancelFact),
}

impl ContinuousExecutionFact {
    pub(super) const fn candidate_key(&self) -> &super::P2CandidateKey {
        &self.candidate_key
    }

    pub(super) const fn sealed_index(&self) -> u64 {
        self.sealed_index
    }

    pub(super) const fn allocated_order_id(&self) -> Option<OrderId> {
        self.allocated_order_id
    }

    pub(super) const fn outcome(&self) -> &ContinuousExecutionOutcome {
        &self.outcome
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ContinuousTradeFact {
    pub(super) stock: StockCode,
    pub(super) triggering_sealed_index: u64,
    pub(super) stock_local_trade_event_index: u64,
    pub(super) trade: Trade,
}

#[derive(Debug)]
pub(super) struct ContinuousStockOutput {
    pub(super) market: Market,
    // Complete P3 allocation journal for this worker: every place draft is present even when P4
    // rejects or terminates it. Downstream must create all of these ledger rows, apply `receipts`,
    // and only then remove `terminal_keys`; filtering terminal drafts here would break conservation.
    pub(super) created_envelopes: Vec<Envelope>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) terminal_keys: Vec<EnvelopeKey>,
    pub(super) trades: Vec<ContinuousTradeFact>,
    pub(super) place_facts: Vec<ContinuousPlaceFact>,
    pub(super) cancel_facts: Vec<ContinuousCancelFact>,
}

#[derive(Debug)]
pub(super) struct ContinuousStockStepOutput {
    pub(super) output: ContinuousStockOutput,
    pub(super) execution_facts: Vec<ContinuousExecutionFact>,
    pub(super) live_envelopes: Vec<ContinuousEnvelopeSnapshot>,
    pub(super) next_trade_event_index: u64,
    pub(super) ledger: EnvelopeLedger,
    pub(super) acceptance_quotes: BTreeMap<u64, ContinuousAcceptanceQuote>,
    #[cfg(feature = "simulation-diagnostics")]
    pub(super) operation_quotes: BTreeMap<u64, ContinuousOperationQuotes>,
}

/// The exact post-operation market observed when this limit order became resting. Later
/// operations in the same stock batch may change the quote or fill the order completely.
#[derive(Clone, Debug)]
pub(super) struct ContinuousAcceptanceQuote {
    pub(super) order: crate::Order,
    pub(super) last_price: Money,
    pub(super) best_bid: Option<Money>,
    pub(super) best_ask: Option<Money>,
}

/// Exact order-book observations around one successfully applied P4 operation.
///
/// This is carried separately from `ContinuousAcceptanceQuote`: the latter is the post-only
/// working-order snapshot used by NPC lifecycle reconciliation, while diagnostics require both
/// sides of every successful place/cancel operation, including immediately filled market orders.
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(feature = "simulation-diagnostics")]
pub(super) struct ContinuousQuoteSnapshot {
    pub(super) bid_cents: Option<i64>,
    pub(super) ask_cents: Option<i64>,
    pub(super) bid_depth: u64,
    pub(super) ask_depth: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(feature = "simulation-diagnostics")]
pub(super) struct ContinuousOperationQuotes {
    pub(super) before: ContinuousQuoteSnapshot,
    pub(super) after: ContinuousQuoteSnapshot,
}

pub(super) fn process_continuous_stock(
    input: ContinuousStockInput,
) -> Result<ContinuousStockOutput, StepFatal> {
    Ok(process_continuous_stock_step(input, false, 0)?.output)
}

pub(super) fn process_continuous_stock_step(
    input: ContinuousStockInput,
    allow_incremental_envelopes: bool,
    next_trade_event_index: u64,
) -> Result<ContinuousStockStepOutput, StepFatal> {
    process_continuous_stock_step_inner(
        input,
        allow_incremental_envelopes,
        next_trade_event_index,
        None,
    )
}

pub(super) fn process_continuous_stock_step_with_ledger(
    input: ContinuousStockInput,
    allow_incremental_envelopes: bool,
    next_trade_event_index: u64,
    ledger: EnvelopeLedger,
) -> Result<ContinuousStockStepOutput, StepFatal> {
    process_continuous_stock_step_inner(
        input,
        allow_incremental_envelopes,
        next_trade_event_index,
        Some(ledger),
    )
}

fn process_continuous_stock_step_inner(
    input: ContinuousStockInput,
    allow_incremental_envelopes: bool,
    next_trade_event_index: u64,
    prior_ledger: Option<EnvelopeLedger>,
) -> Result<ContinuousStockStepOutput, StepFatal> {
    validate_sealed_order(&input.operations)?;
    validate_initial_snapshots(&input.market, &input.envelopes, allow_incremental_envelopes)?;

    let created_envelopes: Vec<_> = input
        .operations
        .iter()
        .filter_map(|operation| match operation {
            P3ValidatedOperation::Place(draft) => Some(draft.materialize_envelope()),
            P3ValidatedOperation::Cancel { .. } => None,
        })
        .collect();
    for envelope in &created_envelopes {
        envelope.validate()?;
    }
    let mut ledger = if let Some(mut ledger) = prior_ledger {
        if ledger_snapshots(&ledger) != input.envelopes {
            return Err(invariant(
                "incremental continuous snapshots disagree with their persistent ledger",
            ));
        }
        ledger.insert_created(created_envelopes.iter().cloned())?;
        ledger
    } else {
        let ledger_envelopes = input
            .envelopes
            .iter()
            .map(|snapshot| snapshot.envelope.clone())
            .chain(created_envelopes.iter().cloned());
        EnvelopeLedger::new(0, ledger_envelopes)?
    };
    let mut market = input.market;
    let mut output = ContinuousStockOutput {
        market: market.clone(),
        created_envelopes,
        receipts: Vec::new(),
        terminal_keys: Vec::new(),
        trades: Vec::new(),
        place_facts: Vec::new(),
        cancel_facts: Vec::new(),
    };
    let mut execution_facts = Vec::new();
    let mut acceptance_quotes = BTreeMap::new();
    #[cfg(feature = "simulation-diagnostics")]
    let mut operation_quotes = BTreeMap::new();
    let mut next_trade_event_index = next_trade_event_index;

    for operation in input.operations {
        #[cfg(feature = "simulation-diagnostics")]
        let quote_before = quote_snapshot(&market)?;
        match operation {
            P3ValidatedOperation::Cancel {
                candidate_key,
                sealed_index,
                account,
                code,
                order_id,
            } => {
                if input.phase == TradingPhase::PreOpen {
                    let fact = ContinuousCancelFact::Rejected {
                        sealed_index,
                        account,
                        code,
                        order_id,
                        reason: ContinuousCancelRejection::AuctionOrderNotCancelable,
                    };
                    output.cancel_facts.push(fact.clone());
                    execution_facts.push(ContinuousExecutionFact {
                        candidate_key,
                        sealed_index,
                        allocated_order_id: None,
                        outcome: ContinuousExecutionOutcome::Cancel(fact),
                    });
                    continue;
                }
                if input.phase != TradingPhase::Continuous {
                    return Err(invariant(
                        "continuous cancellation was routed outside continuous trading",
                    ));
                }
                let cancel = cancel_continuous_order(ContinuousCancelInput {
                    market: market.clone(),
                    envelopes: ledger_snapshots(&ledger),
                    operation: ContinuousCancelOperation {
                        sealed_index,
                        account,
                        code,
                        order_id,
                    },
                })?;
                market = cancel.market;
                if let Some(receipt) = cancel.receipt {
                    let terminal = cancel
                        .terminal_key
                        .ok_or_else(|| invariant("cancel receipt has no terminal envelope key"))?;
                    validate_and_apply(
                        &mut ledger,
                        std::slice::from_ref(&receipt),
                        &[terminal.clone()],
                    )?;
                    output.receipts.push(receipt);
                    output.terminal_keys.push(terminal);
                } else if cancel.terminal_key.is_some() {
                    return Err(invariant("rejected cancellation exposes a terminal key"));
                }
                let fact = cancel.fact;
                #[cfg(feature = "simulation-diagnostics")]
                if matches!(fact, ContinuousCancelFact::Canceled { .. }) {
                    insert_operation_quotes(
                        &mut operation_quotes,
                        sealed_index,
                        quote_before,
                        quote_snapshot(&market)?,
                    )?;
                }
                output.cancel_facts.push(fact.clone());
                execution_facts.push(ContinuousExecutionFact {
                    candidate_key,
                    sealed_index,
                    allocated_order_id: None,
                    outcome: ContinuousExecutionOutcome::Cancel(fact),
                });
            }
            P3ValidatedOperation::Place(draft) => {
                if let Some(reason) = place_phase_rejection(input.phase, draft.kind())? {
                    reject_place(
                        &draft,
                        reason,
                        &mut ledger,
                        &mut output,
                        &mut execution_facts,
                    )?;
                    continue;
                }
                if draft.code() != market.code() {
                    reject_place(
                        &draft,
                        RejectionReason::UnknownStock,
                        &mut ledger,
                        &mut output,
                        &mut execution_facts,
                    )?;
                    continue;
                }
                if draft.kind() == P3PlaceKind::Limit {
                    let bound = market
                        .continuous_limit_bound(draft.side())
                        .map_err(|error| invariant(&error.to_string()))?;
                    let outside = match draft.side() {
                        Side::Buy => draft.limit() > bound,
                        Side::Sell => draft.limit() < bound,
                    };
                    if outside {
                        reject_place(
                            &draft,
                            RejectionReason::PriceCageExceeded,
                            &mut ledger,
                            &mut output,
                            &mut execution_facts,
                        )?;
                        continue;
                    }
                }

                let mut candidate = market.clone();
                let result = match candidate.place(crate::Order {
                    id: draft.order_id(),
                    side: draft.side(),
                    price: draft.limit(),
                    qty: draft.qty(),
                    original_qty: draft.qty(),
                    filled_qty: 0,
                    filled_value: Money::ZERO,
                    owner: draft.owner(),
                    seq: 0,
                }) {
                    Ok(result) => result,
                    Err(MarketError::LimitExceeded { .. }) => {
                        reject_place(
                            &draft,
                            RejectionReason::LimitExceeded,
                            &mut ledger,
                            &mut output,
                            &mut execution_facts,
                        )?;
                        continue;
                    }
                    Err(error) => return Err(invariant(&error.to_string())),
                };
                let trades = result.trades;
                let resting = result.resting;
                if draft.kind() == P3PlaceKind::Market && resting.is_some() {
                    candidate
                        .cancel(draft.order_id())
                        .map_err(|error| invariant(&error.to_string()))?;
                }

                let (mut receipts, states, mut ordinals) =
                    fill_receipts(&draft, &trades, &ledger, &input.config)?;
                let mut terminals = terminal_fill_keys(&receipts);
                if draft.kind() == P3PlaceKind::Market {
                    let incoming = states
                        .get(draft.key())
                        .ok_or_else(|| invariant("market order has no post-fill envelope"))?;
                    if incoming.live() != ResVec::ZERO {
                        let ordinal = take_ordinal(&mut ordinals, draft.key())?;
                        receipts.push(terminal_receipt(
                            draft.sealed_index(),
                            incoming,
                            ReceiptKind::Release,
                            ordinal,
                        )?);
                    }
                    terminals.insert(draft.key().clone());
                }
                let terminal_keys: Vec<_> = terminals.into_iter().collect();
                validate_and_apply(&mut ledger, &receipts, &terminal_keys)?;
                output.receipts.extend(receipts);
                output.terminal_keys.extend(terminal_keys);
                append_trade_facts(
                    draft.code(),
                    draft.sealed_index(),
                    &trades,
                    &mut next_trade_event_index,
                    &mut output.trades,
                )?;
                market = candidate;
                #[cfg(feature = "simulation-diagnostics")]
                insert_operation_quotes(
                    &mut operation_quotes,
                    draft.sealed_index(),
                    quote_before,
                    quote_snapshot(&market)?,
                )?;

                if draft.kind() == P3PlaceKind::Limit {
                    if let Some(resting) = resting {
                        let quote = ContinuousAcceptanceQuote {
                            order: resting.clone(),
                            last_price: market.last_price(),
                            best_bid: market.best_bid(),
                            best_ask: market.best_ask(),
                        };
                        if acceptance_quotes
                            .insert(draft.sealed_index(), quote)
                            .is_some()
                        {
                            return Err(invariant("duplicate sealed acceptance quote"));
                        }
                        output.place_facts.push(ContinuousPlaceFact::Resting {
                            sealed_index: draft.sealed_index(),
                            account: draft.owner(),
                            code: draft.code().clone(),
                            order_id: draft.order_id(),
                            side: draft.side(),
                            price: draft.limit(),
                            remaining_qty: resting.qty,
                        });
                        push_place_execution_fact(&draft, &output, &mut execution_facts)?;
                        continue;
                    }
                }
                let filled_qty = trades.iter().try_fold(0_u32, |total, trade| {
                    total
                        .checked_add(trade.qty)
                        .ok_or_else(|| invariant("incoming filled quantity overflow"))
                })?;
                output.place_facts.push(ContinuousPlaceFact::Filled {
                    sealed_index: draft.sealed_index(),
                    account: draft.owner(),
                    code: draft.code().clone(),
                    order_id: draft.order_id(),
                    side: draft.side(),
                    filled_qty,
                });
                push_place_execution_fact(&draft, &output, &mut execution_facts)?;
            }
        }
    }
    validate_account_fact_identities(&output.place_facts, &output.cancel_facts)?;
    validate_execution_facts(&execution_facts)?;
    output.market = market;
    let live_envelopes = ledger_snapshots(&ledger);
    Ok(ContinuousStockStepOutput {
        live_envelopes,
        output,
        execution_facts,
        next_trade_event_index,
        ledger,
        acceptance_quotes,
        #[cfg(feature = "simulation-diagnostics")]
        operation_quotes,
    })
}

#[cfg(feature = "simulation-diagnostics")]
fn quote_snapshot(market: &Market) -> Result<ContinuousQuoteSnapshot, StepFatal> {
    let bid_depth = market
        .bid_depth()
        .into_iter()
        .try_fold(0_u64, |total, (_, qty)| {
            total
                .checked_add(qty)
                .ok_or_else(|| invariant("continuous bid depth overflow"))
        })?;
    let ask_depth = market
        .ask_depth()
        .into_iter()
        .try_fold(0_u64, |total, (_, qty)| {
            total
                .checked_add(qty)
                .ok_or_else(|| invariant("continuous ask depth overflow"))
        })?;
    Ok(ContinuousQuoteSnapshot {
        bid_cents: market.best_bid().map(|price| price.cents()),
        ask_cents: market.best_ask().map(|price| price.cents()),
        bid_depth,
        ask_depth,
    })
}

#[cfg(feature = "simulation-diagnostics")]
fn insert_operation_quotes(
    quotes: &mut BTreeMap<u64, ContinuousOperationQuotes>,
    sealed_index: u64,
    before: ContinuousQuoteSnapshot,
    after: ContinuousQuoteSnapshot,
) -> Result<(), StepFatal> {
    if quotes
        .insert(sealed_index, ContinuousOperationQuotes { before, after })
        .is_some()
    {
        return Err(invariant("duplicate sealed operation quote"));
    }
    Ok(())
}

pub(super) fn append_trade_facts(
    stock: &StockCode,
    triggering_sealed_index: u64,
    trades: &[Trade],
    next_index: &mut u64,
    output: &mut Vec<ContinuousTradeFact>,
) -> Result<(), StepFatal> {
    let count = u64::try_from(trades.len())
        .map_err(|_| invariant("trade fact count exceeds u64 identity space"))?;
    let next_after = next_index
        .checked_add(count)
        .ok_or_else(|| invariant("stock-local trade event index overflow"))?;
    let mut staged = Vec::with_capacity(trades.len());
    let mut cursor = *next_index;
    for trade in trades {
        staged.push(ContinuousTradeFact {
            stock: stock.clone(),
            triggering_sealed_index,
            stock_local_trade_event_index: cursor,
            trade: trade.clone(),
        });
        cursor = cursor
            .checked_add(1)
            .ok_or_else(|| invariant("stock-local trade event index overflow"))?;
    }
    if cursor != next_after {
        return Err(invariant("stock-local trade event cursor drift"));
    }
    output.extend(staged);
    *next_index = next_after;
    Ok(())
}

fn validate_account_fact_identities(
    places: &[ContinuousPlaceFact],
    cancels: &[ContinuousCancelFact],
) -> Result<(), StepFatal> {
    let mut seen = BTreeSet::new();
    for (account, sealed_index) in places
        .iter()
        .map(|fact| match fact {
            ContinuousPlaceFact::Resting {
                account,
                sealed_index,
                ..
            }
            | ContinuousPlaceFact::Filled {
                account,
                sealed_index,
                ..
            }
            | ContinuousPlaceFact::Rejected {
                account,
                sealed_index,
                ..
            } => (*account, *sealed_index),
        })
        .chain(cancels.iter().map(|fact| match fact {
            ContinuousCancelFact::Canceled {
                account,
                sealed_index,
                ..
            }
            | ContinuousCancelFact::Rejected {
                account,
                sealed_index,
                ..
            } => (*account, *sealed_index),
        }))
    {
        if !seen.insert((account, sealed_index)) {
            return Err(invariant(
                "one account operation emitted more than one sealed fact identity",
            ));
        }
    }
    Ok(())
}

fn validate_execution_facts(facts: &[ContinuousExecutionFact]) -> Result<(), StepFatal> {
    let mut candidate_keys = BTreeSet::new();
    let mut sealed_indices = BTreeSet::new();
    for fact in facts {
        if !candidate_keys.insert(fact.candidate_key.clone()) {
            return Err(invariant(
                "one candidate emitted more than one continuous execution fact",
            ));
        }
        if !sealed_indices.insert(fact.sealed_index) {
            return Err(invariant(
                "one sealed identity emitted more than one continuous execution fact",
            ));
        }
        match &fact.outcome {
            ContinuousExecutionOutcome::Place { fact: place, .. } => {
                let (sealed_index, order_id) = match place {
                    ContinuousPlaceFact::Resting {
                        sealed_index,
                        order_id,
                        ..
                    }
                    | ContinuousPlaceFact::Filled {
                        sealed_index,
                        order_id,
                        ..
                    }
                    | ContinuousPlaceFact::Rejected {
                        sealed_index,
                        order_id,
                        ..
                    } => (*sealed_index, *order_id),
                };
                if sealed_index != fact.sealed_index || fact.allocated_order_id != Some(order_id) {
                    return Err(invariant(
                        "place execution fact identity disagrees with its typed outcome",
                    ));
                }
            }
            ContinuousExecutionOutcome::Cancel(cancel) => {
                let sealed_index = match cancel {
                    ContinuousCancelFact::Canceled { sealed_index, .. }
                    | ContinuousCancelFact::Rejected { sealed_index, .. } => *sealed_index,
                };
                if sealed_index != fact.sealed_index || fact.allocated_order_id.is_some() {
                    return Err(invariant(
                        "cancel execution fact identity disagrees with its typed outcome",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_sealed_order(operations: &[P3ValidatedOperation]) -> Result<(), StepFatal> {
    if operations
        .windows(2)
        .any(|pair| pair[0].sealed_index() >= pair[1].sealed_index())
    {
        return Err(invariant(
            "continuous stock operations are not in strict sealed order",
        ));
    }
    Ok(())
}

fn validate_initial_snapshots(
    market: &Market,
    snapshots: &[ContinuousEnvelopeSnapshot],
    allow_incremental_envelopes: bool,
) -> Result<(), StepFatal> {
    let orders = market.resting_orders();
    let mut by_order = BTreeMap::new();
    for snapshot in snapshots {
        snapshot.envelope.validate()?;
        if snapshot.envelope.origin() != EnvelopeOrigin::TickStart
            && !(allow_incremental_envelopes
                && snapshot.envelope.origin() == EnvelopeOrigin::P3Created)
        {
            return Err(invariant(
                "post-P0 continuous input contains a same-tick envelope",
            ));
        }
        if snapshot.envelope.key().stock != *market.code() {
            return Err(invariant("continuous envelope belongs to another stock"));
        }
        let key = snapshot.envelope.key().order;
        if by_order.insert(key, snapshot).is_some() {
            return Err(invariant("duplicate continuous envelope snapshot"));
        }
    }
    for order in &orders {
        let snapshot = by_order
            .remove(&order.id)
            .ok_or_else(|| invariant("resting order has no continuous envelope snapshot"))?;
        validate_snapshot(snapshot, order)?;
    }
    if !by_order.is_empty() {
        return Err(invariant(
            "continuous envelope snapshot has no resting order",
        ));
    }
    Ok(())
}

fn place_phase_rejection(
    phase: TradingPhase,
    kind: P3PlaceKind,
) -> Result<Option<RejectionReason>, StepFatal> {
    match phase {
        TradingPhase::Continuous => Ok(None),
        TradingPhase::PreOpen => Ok(Some(RejectionReason::AuctionOrderEntryClosed)),
        TradingPhase::CallAuction | TradingPhase::ClosingAuction if kind == P3PlaceKind::Market => {
            Ok(Some(RejectionReason::AuctionLimitOrderRequired))
        }
        TradingPhase::CallAuction | TradingPhase::ClosingAuction => Err(invariant(
            "auction limit order was routed to the continuous stock worker",
        )),
    }
}

fn reject_place(
    draft: &super::EnvelopeDraft,
    reason: RejectionReason,
    ledger: &mut EnvelopeLedger,
    output: &mut ContinuousStockOutput,
    execution_facts: &mut Vec<ContinuousExecutionFact>,
) -> Result<(), StepFatal> {
    let envelope = ledger.get(draft.key())?.clone();
    let receipt = terminal_receipt(draft.sealed_index(), &envelope, ReceiptKind::Reject, 0)?;
    let terminal = draft.key().clone();
    validate_and_apply(
        ledger,
        std::slice::from_ref(&receipt),
        std::slice::from_ref(&terminal),
    )?;
    output.receipts.push(receipt);
    output.terminal_keys.push(terminal);
    output.place_facts.push(ContinuousPlaceFact::Rejected {
        sealed_index: draft.sealed_index(),
        account: draft.owner(),
        code: draft.code().clone(),
        order_id: draft.order_id(),
        reason,
    });
    push_place_execution_fact(draft, output, execution_facts)?;
    Ok(())
}

fn push_place_execution_fact(
    draft: &super::EnvelopeDraft,
    output: &ContinuousStockOutput,
    execution_facts: &mut Vec<ContinuousExecutionFact>,
) -> Result<(), StepFatal> {
    let fact = output
        .place_facts
        .last()
        .ok_or_else(|| invariant("place operation produced no typed place fact"))?
        .clone();
    execution_facts.push(ContinuousExecutionFact {
        candidate_key: draft.candidate_key().clone(),
        sealed_index: draft.sealed_index(),
        allocated_order_id: Some(draft.order_id()),
        outcome: ContinuousExecutionOutcome::Place {
            fact,
            original_qty: draft.qty(),
        },
    });
    Ok(())
}

fn fill_receipts(
    draft: &super::EnvelopeDraft,
    trades: &[Trade],
    ledger: &EnvelopeLedger,
    config: &GameConfig,
) -> Result<
    (
        Vec<EnvelopeReceipt>,
        BTreeMap<EnvelopeKey, Envelope>,
        BTreeMap<EnvelopeKey, u64>,
    ),
    StepFatal,
> {
    let mut states: BTreeMap<_, _> = ledger
        .iter()
        .map(|(key, envelope)| (key.clone(), envelope.clone()))
        .collect();
    let mut ordinals = BTreeMap::new();
    let mut receipts = Vec::with_capacity(trades.len().saturating_mul(2));
    for trade in trades {
        let gross = trade
            .price
            .mul_shares(trade.qty)
            .map_err(|error| invariant(&error.to_string()))?;
        let (buyer, buyer_before, seller, seller_before) = match draft.side() {
            Side::Buy => (
                draft.key().clone(),
                trade.taker_filled_value_before,
                EnvelopeKey {
                    account: trade.maker,
                    stock: draft.code().clone(),
                    order: trade.maker_order_id,
                    side: Side::Sell,
                },
                trade.maker_filled_value_before,
            ),
            Side::Sell => (
                EnvelopeKey {
                    account: trade.maker,
                    stock: draft.code().clone(),
                    order: trade.maker_order_id,
                    side: Side::Buy,
                },
                trade.maker_filled_value_before,
                draft.key().clone(),
                trade.taker_filled_value_before,
            ),
        };
        receipts.push(fill_receipt(
            draft.sealed_index(),
            buyer,
            buyer_before,
            trade.qty,
            gross,
            &mut states,
            &mut ordinals,
            config,
        )?);
        receipts.push(fill_receipt(
            draft.sealed_index(),
            seller,
            seller_before,
            trade.qty,
            gross,
            &mut states,
            &mut ordinals,
            config,
        )?);
    }
    Ok((receipts, states, ordinals))
}

#[allow(clippy::too_many_arguments)]
fn fill_receipt(
    sealed_index: u64,
    key: EnvelopeKey,
    filled_value_before: Money,
    fill_qty: u32,
    gross: Money,
    states: &mut BTreeMap<EnvelopeKey, Envelope>,
    ordinals: &mut BTreeMap<EnvelopeKey, u64>,
    config: &GameConfig,
) -> Result<EnvelopeReceipt, StepFatal> {
    let envelope = states
        .get_mut(&key)
        .ok_or_else(|| invariant("trade participant has no live envelope"))?;
    let audit = envelope.audit();
    if audit.filled_value != filled_value_before {
        return Err(invariant(
            "trade filled-value history disagrees with envelope audit",
        ));
    }
    let remaining_qty_after = audit
        .remaining_qty
        .checked_sub(fill_qty)
        .ok_or_else(|| invariant("trade fill exceeds envelope remaining quantity"))?;
    let value_after = audit
        .filled_value
        .add(gross)
        .map_err(|error| invariant(&error.to_string()))?;
    let transition = match key.side {
        Side::Buy => FillTransition::buy(BuyFillInput {
            config,
            limit: audit.limit,
            fill_qty,
            remaining_qty_after,
            filled_value_before: audit.filled_value,
            gross_delta: gross,
            live_before: envelope.live(),
        })?,
        Side::Sell => FillTransition::sell(SellFillInput {
            config,
            fill_qty,
            remaining_qty_after,
            filled_value_before: audit.filled_value,
            gross_delta: gross,
            nominal_before: audit.nominal,
            charged_before: audit.charged,
        })?,
    };
    let ordinal = take_ordinal(ordinals, &key)?;
    let receipt = EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(sealed_index),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal,
            },
        )?,
        envelope: key,
        kind: ReceiptKind::Fill,
        qty_before: audit.remaining_qty,
        qty_after: remaining_qty_after,
        value_before: audit.filled_value,
        value_after,
        delta: transition.delta,
        nominal: transition.nominal,
        charged: transition.charged,
        charged_before: audit.charged,
        charged_after: transition.charged_after,
        deliver_qty: transition.deliver_qty,
        deliver_cash: transition.deliver_cash,
    };
    let filled_qty = audit
        .filled_qty
        .checked_add(fill_qty)
        .ok_or_else(|| invariant("envelope cumulative filled quantity overflow"))?;
    envelope.apply(
        transition.delta,
        EnvelopeAudit {
            limit: audit.limit,
            remaining_qty: remaining_qty_after,
            filled_qty,
            filled_value: value_after,
            nominal: transition.nominal_after,
            charged: transition.charged_after,
        },
    )?;
    Ok(receipt)
}

fn take_ordinal(
    ordinals: &mut BTreeMap<EnvelopeKey, u64>,
    key: &EnvelopeKey,
) -> Result<u64, StepFatal> {
    let ordinal = ordinals.entry(key.clone()).or_insert(0);
    let current = *ordinal;
    *ordinal = ordinal
        .checked_add(1)
        .ok_or_else(|| invariant("receipt transition ordinal overflow"))?;
    Ok(current)
}

fn terminal_fill_keys(receipts: &[EnvelopeReceipt]) -> BTreeSet<EnvelopeKey> {
    receipts
        .iter()
        .filter(|receipt| receipt.kind == ReceiptKind::Fill && receipt.qty_after == 0)
        .map(|receipt| receipt.envelope.clone())
        .collect()
}

fn terminal_receipt(
    sealed_index: u64,
    envelope: &Envelope,
    kind: ReceiptKind,
    ordinal: u64,
) -> Result<EnvelopeReceipt, StepFatal> {
    let audit = envelope.audit();
    Ok(EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(sealed_index),
            ReceiptTransition {
                envelope: envelope.key().clone(),
                ordinal,
            },
        )?,
        envelope: envelope.key().clone(),
        kind,
        qty_before: audit.remaining_qty,
        qty_after: audit.remaining_qty,
        value_before: audit.filled_value,
        value_after: audit.filled_value,
        delta: ReceiptDelta::sealed(ResVec::ZERO, envelope.live(), ResVec::ZERO),
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: audit.charged,
        charged_after: audit.charged,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    })
}

pub(super) fn ledger_snapshots(ledger: &EnvelopeLedger) -> Vec<ContinuousEnvelopeSnapshot> {
    ledger
        .iter()
        .map(|(_, envelope)| ContinuousEnvelopeSnapshot {
            envelope: envelope.clone(),
            audit: envelope.audit(),
        })
        .collect()
}

fn validate_and_apply(
    ledger: &mut EnvelopeLedger,
    receipts: &[EnvelopeReceipt],
    terminal_keys: &[EnvelopeKey],
) -> Result<(), StepFatal> {
    let mut validation = receipts.to_vec();
    ledger.apply(&mut validation)?;
    ledger.remove_terminal(terminal_keys)
}

pub(super) fn cancel_continuous_order(
    input: ContinuousCancelInput,
) -> Result<ContinuousCancelOutput, StepFatal> {
    let ContinuousCancelInput {
        market,
        envelopes,
        operation,
    } = input;
    if market.code() != &operation.code {
        return Ok(rejected(
            market,
            operation,
            ContinuousCancelRejection::UnknownStock,
        ));
    }

    let mut candidate = market.clone();
    let order = match candidate.cancel(operation.order_id) {
        Ok(order) => order,
        Err(MarketError::OrderBook(OrderError::OrderNotFound(_))) => {
            return Ok(rejected(
                market,
                operation,
                ContinuousCancelRejection::OrderNotFound,
            ));
        }
        Err(error) => return Err(invariant(&error.to_string())),
    };
    if order.owner != operation.account {
        return Ok(rejected(
            market,
            operation,
            ContinuousCancelRejection::NotOrderOwner,
        ));
    }

    let mut matching = envelopes.iter().filter(|snapshot| {
        snapshot.envelope.key().stock == operation.code
            && snapshot.envelope.key().order == operation.order_id
    });
    let snapshot = matching
        .next()
        .ok_or_else(|| invariant("canceled order has no live envelope"))?;
    if matching.next().is_some() {
        return Err(invariant("canceled order has duplicate live envelopes"));
    }
    validate_snapshot(snapshot, &order)?;
    if snapshot.envelope.origin() == EnvelopeOrigin::P3Created {
        return Ok(rejected(
            market,
            operation,
            ContinuousCancelRejection::SameTickEnvelope,
        ));
    }

    let key = snapshot.envelope.key().clone();
    let receipt = release_receipt(&operation, snapshot)?;
    Ok(ContinuousCancelOutput {
        market: candidate,
        receipt: Some(receipt),
        terminal_key: Some(key),
        fact: ContinuousCancelFact::Canceled {
            sealed_index: operation.sealed_index,
            account: operation.account,
            code: operation.code,
            order_id: operation.order_id,
            side: order.side,
            remaining_qty: order.qty,
        },
    })
}

fn release_receipt(
    operation: &ContinuousCancelOperation,
    snapshot: &ContinuousEnvelopeSnapshot,
) -> Result<EnvelopeReceipt, StepFatal> {
    let key = snapshot.envelope.key().clone();
    Ok(EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(operation.sealed_index),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )?,
        envelope: key,
        kind: ReceiptKind::Release,
        qty_before: snapshot.audit.remaining_qty,
        qty_after: snapshot.audit.remaining_qty,
        value_before: snapshot.audit.filled_value,
        value_after: snapshot.audit.filled_value,
        delta: ReceiptDelta::sealed(ResVec::ZERO, snapshot.envelope.live(), ResVec::ZERO),
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: snapshot.audit.charged,
        charged_after: snapshot.audit.charged,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    })
}

fn validate_snapshot(
    snapshot: &ContinuousEnvelopeSnapshot,
    order: &crate::Order,
) -> Result<(), StepFatal> {
    let key = snapshot.envelope.key();
    if snapshot.envelope.audit() != snapshot.audit {
        return Err(invariant("live envelope and supplied audit disagree"));
    }
    if key.account != order.owner
        || key.order != order.id
        || key.side != order.side
        || snapshot.audit.limit != order.price
        || snapshot.audit.remaining_qty != order.qty
        || snapshot.audit.filled_qty != order.filled_qty
        || snapshot.audit.filled_value != order.filled_value
    {
        return Err(invariant(
            "live envelope audit disagrees with canceled order",
        ));
    }
    Ok(())
}

fn rejected(
    market: Market,
    operation: ContinuousCancelOperation,
    reason: ContinuousCancelRejection,
) -> ContinuousCancelOutput {
    ContinuousCancelOutput {
        market,
        receipt: None,
        terminal_key: None,
        fact: ContinuousCancelFact::Rejected {
            sealed_index: operation.sealed_index,
            account: operation.account,
            code: operation.code,
            order_id: operation.order_id,
            reason,
        },
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p4_continuous".to_owned(),
    }
}
