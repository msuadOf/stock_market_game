//! 将 Continuous 股票 worker 事实投影为 Projection 事件事实。
//!
//! 稳定身份均来自 typed 股票处理事实，不从集合位置或 worker completion 时点虚构。
//! 本模块不分配外部可见事件序号。

use super::{
    continuous_matching::{
        ContinuousCancelFact, ContinuousCancelRejection, ContinuousExecutionFact,
        ContinuousExecutionOutcome, ContinuousPlaceFact, ContinuousTradeFact,
    },
    event_collection::OwnedEventFact,
    EventStableKey, StepFatal,
};

/// Projects the continuation-facing typed operation facts into the existing Projection event adapter.
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

/// 把 Continuous 股票处理事实投影为 Projection 拥有的事件事实。
///
/// ContinuousPlaceFact::Filled 不直接产生公共事件；每次成交由 ContinuousTradeFact 恰好表示一次。
/// 触发成交的 sealed identity 保留在真实股票处理 audit fact 上；
/// ADR-0017 的 Trade stable key 使用显式逐股 trade index。
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
        ContinuousCancelRejection::OrderAlreadyFilled => RejectionReason::OrderAlreadyFilled,
        ContinuousCancelRejection::NotOrderOwner => RejectionReason::NotOrderOwner,
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
                description: "Continuous StockProcessing 事实含有重复 EventStableKey".to_owned(),
                location: "pipeline::execution_fact_producers".to_owned(),
            });
        }
    }
    Ok(())
}
