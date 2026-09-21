//! P7 projection of continuous-stock worker facts.
//!
//! Every stable identity comes from the typed P4 fact. This module never uses
//! collection position or worker completion timing to invent an identity, and it
//! never assigns the externally visible event sequence.

use super::{
    p4_continuous::{
        ContinuousCancelFact, ContinuousCancelRejection, ContinuousExecutionFact,
        ContinuousExecutionOutcome, ContinuousPlaceFact, ContinuousTradeFact,
    },
    p7_events::OwnedEventFact,
    EventStableKey, StepFatal,
};

/// Projects the continuation-facing typed operation facts into the existing P7 event adapter.
///
/// This is primarily needed for an unknown-stock cancellation, which has no stock worker output.
/// Known-stock worker facts continue through `adapt_continuous_facts` at the single finish seam.
pub(super) fn adapt_continuous_execution_facts(
    executions: &[ContinuousExecutionFact],
) -> Result<Vec<OwnedEventFact>, StepFatal> {
    let mut places = Vec::new();
    let mut cancels = Vec::new();
    for execution in executions {
        match &execution.outcome {
            ContinuousExecutionOutcome::Place { fact, .. } => places.push(fact.clone()),
            ContinuousExecutionOutcome::Cancel(fact) => cancels.push(fact.clone()),
        }
    }
    adapt_continuous_facts(&places, &cancels, &[])
}
use crate::{Event, RejectionReason};
use std::collections::BTreeSet;

/// Projects P4 continuous facts into P7-owned event facts.
///
/// `ContinuousPlaceFact::Filled` deliberately has no direct public event: each public fill is
/// represented exactly once by `ContinuousTradeFact`. The triggering sealed identity stays on
/// that P4 audit fact; the ADR-0017 Trade stable key uses its explicit stock-local trade index.
pub(super) fn adapt_continuous_facts(
    places: &[ContinuousPlaceFact],
    cancels: &[ContinuousCancelFact],
    trades: &[ContinuousTradeFact],
) -> Result<Vec<OwnedEventFact>, StepFatal> {
    let mut facts = Vec::new();
    for place in places {
        match place {
            ContinuousPlaceFact::Resting {
                sealed_index,
                account,
                code,
                order_id,
                side,
                price,
                remaining_qty,
            } => push_fact(
                &mut facts,
                Event::OrderAccepted {
                    seq: 0,
                    account: *account,
                    code: code.clone(),
                    id: *order_id,
                    side: *side,
                    price: *price,
                    remaining_qty: *remaining_qty,
                },
                *sealed_index,
            ),
            ContinuousPlaceFact::Filled { .. } => {}
            ContinuousPlaceFact::Rejected {
                sealed_index,
                account,
                code,
                reason,
                ..
            } => push_fact(
                &mut facts,
                Event::IntentRejected {
                    seq: 0,
                    account: *account,
                    code: code.clone(),
                    reason: reason.clone(),
                },
                *sealed_index,
            ),
        }
    }
    for cancel in cancels {
        match cancel {
            ContinuousCancelFact::Canceled {
                sealed_index,
                account,
                code,
                order_id,
                remaining_qty,
                ..
            } => push_fact(
                &mut facts,
                Event::OrderCanceled {
                    seq: 0,
                    account: *account,
                    code: code.clone(),
                    id: *order_id,
                    remaining_qty: *remaining_qty,
                },
                *sealed_index,
            ),
            ContinuousCancelFact::Rejected {
                sealed_index,
                account,
                code,
                reason,
                ..
            } => push_fact(
                &mut facts,
                Event::IntentRejected {
                    seq: 0,
                    account: *account,
                    code: code.clone(),
                    reason: cancellation_reason(*reason),
                },
                *sealed_index,
            ),
        }
    }
    for trade in trades {
        push_fact(
            &mut facts,
            Event::Trade {
                seq: 0,
                code: trade.stock.clone(),
                price: trade.trade.price,
                qty: trade.trade.qty,
                maker: trade.trade.maker,
                taker: trade.trade.taker,
            },
            trade.stock_local_trade_event_index,
        );
    }
    validate_unique_facts(&facts)?;
    Ok(facts)
}

fn push_fact(facts: &mut Vec<OwnedEventFact>, event: Event, local_event_index: u64) {
    facts.push(OwnedEventFact {
        key: EventStableKey::for_event(&event, local_event_index),
        event,
    });
}

fn cancellation_reason(reason: ContinuousCancelRejection) -> RejectionReason {
    match reason {
        ContinuousCancelRejection::UnknownStock => RejectionReason::UnknownStock,
        ContinuousCancelRejection::OrderNotFound => RejectionReason::OrderNotFound,
        ContinuousCancelRejection::NotOrderOwner => RejectionReason::NotOrderOwner,
        ContinuousCancelRejection::SameTickEnvelope => RejectionReason::SameTickOrderNotCancelable,
        ContinuousCancelRejection::AuctionOrderNotCancelable => {
            RejectionReason::AuctionOrderNotCancelable
        }
    }
}

fn validate_unique_facts(facts: &[OwnedEventFact]) -> Result<(), StepFatal> {
    let mut seen = BTreeSet::new();
    for fact in facts {
        if !seen.insert(&fact.key) {
            return Err(StepFatal::InvariantViolation {
                description: "continuous P4 facts contain duplicate event stable identity"
                    .to_owned(),
                location: "pipeline::p7_p4_producers".to_owned(),
            });
        }
    }
    Ok(())
}
