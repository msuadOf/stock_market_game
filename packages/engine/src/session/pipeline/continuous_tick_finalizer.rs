//! Continuous P4 tail and the single ReceiptAggregation-Projection pass, entirely on the Continuous private candidate.

use super::{
    adaptive_plan_chain::PlanChainFactConsumption,
    continuous_lifecycle_projection::project_continuous_retail_lifecycle,
    continuous_matching::IncrementalContinuousStockFinish,
    continuous_tick_transaction::ContinuousTransactionError,
    event_collection::{collect_events, OwnedEventFact},
    execution_fact_producers::{adapt_continuous_execution_facts, adapt_continuous_facts},
    session_execution_transaction::{
        SessionExecutionTransactionError, SessionExecutionTransactionOutput,
    },
    stock_auction::auction_day_end::finalize_trading_day,
    stock_execution_transaction::{
        apply_stock_execution_transaction_with_preceding_beliefs, SettlementApplicationContext,
    },
    AccountValidationOutput, EventStableKey, IntentCandidateBatch, ReceiptSource, StepFatal,
};
use crate::session::{
    completed_market_minute_count, MarketMinuteClose, PendingPlanEvent, RetailOrderDiagnosticEvent,
    GAME_INTRADAY_MINUTES_PER_DAY,
};
use crate::{Event, GameSession, Money, StockCode, TradingPhase};

pub(super) struct ContinuousTickBoundary {
    tick_after: u64,
    pub(super) ends_day: bool,
    completed_minutes: u16,
}

pub(super) struct ContinuousLifecycleProjectionInput<'a> {
    pub(super) candidates: &'a IntentCandidateBatch,
    pub(super) validation: &'a AccountValidationOutput,
    pub(super) consumed: &'a PlanChainFactConsumption,
}

pub(super) struct ContinuousTickFinalizationContext<'a> {
    pub(super) boundary: ContinuousTickBoundary,
    pub(super) day_end_event_base: u64,
    pub(super) lifecycle: ContinuousLifecycleProjectionInput<'a>,
}

impl ContinuousTickBoundary {
    pub(super) fn capture(session: &GameSession) -> Result<Self, StepFatal> {
        if session.phase() != TradingPhase::Continuous {
            return Err(invariant(
                "continuous finalizer requires continuous trading",
            ));
        }
        let tick_after = session
            .tick
            .checked_add(1)
            .ok_or_else(|| invariant("continuous tick overflow"))?;
        let ends_day = tick_after.is_multiple_of(session.setup.ticks_per_day);
        if ends_day && session.day == u32::MAX {
            return Err(invariant("continuous trading day overflow"));
        }
        let continuous_ticks = session
            .setup
            .ticks_per_day
            .checked_sub(session.setup.auction_ticks)
            .and_then(|ticks| ticks.checked_sub(session.setup.closing_auction_ticks))
            .ok_or_else(|| invariant("invalid continuous trading window"))?;
        let completed_ticks =
            session.tick % session.setup.ticks_per_day - session.setup.auction_ticks + 1;
        let completed_minutes = completed_market_minute_count(completed_ticks, continuous_ticks)
            .map_err(|error| {
                invariant(&format!("continuous market-minute mapping failed: {error}"))
            })?;
        Ok(Self {
            tick_after,
            ends_day,
            completed_minutes,
        })
    }
}

/// `session` is the discardable candidate owned by Continuous, never external authority.
/// Price/depth facts were frozen by P4 before the day-end worker cleared its book.
/// Settlement uses the original tick's minute; only after settlement is the clock advanced and T+1 unlocked.
pub(super) fn finalize_continuous_tick(
    session: &mut GameSession,
    finish: IncrementalContinuousStockFinish,
    mut facts: Vec<OwnedEventFact>,
    preceding_receipts: &[super::EnvelopeReceipt],
    context: ContinuousTickFinalizationContext<'_>,
) -> Result<SessionExecutionTransactionOutput, ContinuousTransactionError> {
    let ContinuousTickFinalizationContext {
        boundary,
        day_end_event_base,
        lifecycle,
    } = context;
    let finalize_error = ContinuousTransactionError::Finalization;
    if session.next_receipt_base != session.envelope_ledger.next_receipt_index() {
        return Err(finalize_error(invariant(
            "session and ledger receipt cursors disagree",
        )));
    }
    facts.extend(adapt_continuous_execution_facts(&finish.detached_facts).map_err(finalize_error)?);
    for worker in &finish.workers {
        facts.extend(
            adapt_continuous_facts(&worker.place_facts, &worker.cancel_facts, &worker.trades)
                .map_err(finalize_error)?,
        );
        let code = worker.market.code();
        for trade in &worker.trades {
            checked_update_candle(session, code, trade.trade.price, u64::from(trade.trade.qty))
                .map_err(finalize_error)?;
        }
        let price = finish.prices.get(code).ok_or_else(|| {
            finalize_error(invariant("continuous worker has no frozen closing price"))
        })?;
        let (last, bids, asks) = (price.last, price.bids.clone(), price.asks.clone());
        checked_update_candle(session, code, last, 0).map_err(finalize_error)?;
        let prices = session
            .price_history
            .get_mut(code)
            .ok_or_else(|| finalize_error(invariant("continuous market has no price history")))?;
        prices.push_back(last);
        while prices.len() > session.setup.history_len {
            prices.pop_front();
        }
        let minutes = session
            .market_minute_closes
            .get_mut(code)
            .ok_or_else(|| finalize_error(invariant("continuous market has no minute history")))?;
        let recorded = u16::try_from(minutes.len())
            .map_err(|_| finalize_error(invariant("continuous minute history exceeds u16")))?;
        if recorded > boundary.completed_minutes {
            return Err(finalize_error(invariant(
                "continuous minute history is ahead of tick",
            )));
        }
        let day_start = u64::from(session.day) * u64::from(GAME_INTRADAY_MINUTES_PER_DAY);
        for minute in recorded..boundary.completed_minutes {
            minutes.push(MarketMinuteClose {
                absolute_trading_minute: day_start + u64::from(minute),
                close: last,
            });
        }
        facts.push(owned_event(
            Event::PriceTick {
                seq: 0,
                tick: boundary.tick_after,
                code: code.clone(),
                last_price: last,
                daily_candle: session.active_daily_candles[code].clone(),
                bids,
                asks,
            },
            0,
        ));
    }

    let transaction = apply_stock_execution_transaction_with_preceding_beliefs(
        &session.envelope_ledger,
        &session.accounts,
        &session.retail_experience,
        &session.belief_books,
        &session.retail_projection_seen,
        finish.workers,
        SettlementApplicationContext::new(
            crate::experience::ExperienceMoment {
                civil_date: session.civil_date(),
                market_minute: session.current_market_minute(),
                trading_day: u64::from(session.day),
            },
            preceding_receipts,
            session.setup.t1_enabled,
        ),
    )
    .map_err(|error| {
        ContinuousTransactionError::SessionExecution(
            SessionExecutionTransactionError::StockExecution(error),
        )
    })?;
    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    session.envelope_ledger = transaction.ledger;
    session.next_receipt_base = session.envelope_ledger.next_receipt_index();
    session.accounts.extend(transaction.account_patch);
    session.retail_experience.extend(transaction.retail_patch);
    session.belief_books.extend(transaction.belief_patch);
    session.retail_projection_seen = transaction.seen;
    for (code, stock) in transaction.stocks {
        session.markets.insert(code, stock.market);
    }
    project_continuous_retail_lifecycle(
        session,
        lifecycle.candidates,
        lifecycle.validation,
        &finish.execution_facts,
        &transaction.receipts,
        lifecycle.consumed,
    )
    .map_err(finalize_error)?;
    session.tick = boundary.tick_after;

    if boundary.ends_day {
        #[cfg(feature = "simulation-diagnostics")]
        let day_end_causal_time = {
            let mut time = session.causal_time();
            time.phase = TradingPhase::Continuous;
            time
        };
        let mut releases = transaction
            .receipts
            .iter()
            .filter(|receipt| matches!(receipt.local_key.source(), ReceiptSource::DayEnd(_)))
            .collect::<Vec<_>>();
        releases.sort_by(|left, right| left.envelope.cmp(&right.envelope));
        #[cfg(feature = "simulation-diagnostics")]
        let cleared_quote_codes = releases
            .iter()
            .map(|release| release.envelope.stock.clone())
            .collect::<std::collections::BTreeSet<_>>();
        for (ordinal, release) in releases.into_iter().enumerate() {
            let key = &release.envelope;
            #[cfg(feature = "simulation-diagnostics")]
            session.causal_terminated_at(
                day_end_causal_time,
                (key.account, key.order, release.qty_before),
                &key.stock,
                crate::diagnostics::causal::Termination::DayEnd,
            );
            session.record_parent_order_canceled(key.account, &key.stock, key.order);
            session.remove_npc_order_lifecycle(key.account, &key.stock, key.order);
            if session.retail_experience.contains_key(&key.account) {
                session
                    .last_retail_order_events
                    .push(RetailOrderDiagnosticEvent::Canceled {
                        account: key.account,
                        code: key.stock.clone(),
                        order_id: key.order,
                        remaining_qty: release.qty_before,
                    });
            }
            let local_index = u64::try_from(ordinal)
                .ok()
                .and_then(|ordinal| day_end_event_base.checked_add(ordinal))
                .ok_or_else(|| {
                    finalize_error(invariant("continuous DayEnd event index overflow"))
                })?;
            facts.push(owned_event(
                Event::OrderCanceled {
                    seq: 0,
                    account: key.account,
                    code: key.stock.clone(),
                    id: key.order,
                    remaining_qty: release.qty_before,
                },
                local_index,
            ));
        }
        #[cfg(feature = "simulation-diagnostics")]
        for code in cleared_quote_codes {
            session.causal_snapshot_at(day_end_causal_time, &code);
        }
        let ended = session
            .parent_orders
            .values()
            .flat_map(|plans| plans.values())
            .filter(|parent| parent.filled_qty < parent.target_qty)
            .filter_map(|parent| parent.linked_plan_id)
            .collect::<Vec<_>>();
        session
            .pending_plan_events
            .extend(ended.into_iter().map(|plan_id| PendingPlanEvent::DayEnded {
                plan_id,
                trading_day: u64::from(session.day),
            }));
        facts.extend(finalize_trading_day(session).map_err(|error| {
            finalize_error(invariant(&format!("continuous DayEnd failed: {error}")))
        })?);
    }
    let collected = collect_events(facts, session.seq).map_err(finalize_error)?;
    session.seq = collected.next_seq;
    Ok(SessionExecutionTransactionOutput {
        events: collected.events,
        event_keys: collected.keys,
        receipts: transaction.receipts,
        settlement: transaction.settlement,
    })
}

/// Keep the legacy candle reducer and its first-trade/open semantics; validate its checked
/// arithmetic first so the new tick returns a typed fatal rather than panicking mid-candidate.
fn checked_update_candle(
    session: &mut GameSession,
    code: &StockCode,
    price: Money,
    qty: u64,
) -> Result<(), StepFatal> {
    if qty > 0 {
        let gross = u64::try_from(price.cents())
            .ok()
            .filter(|cents| *cents > 0)
            .and_then(|cents| cents.checked_mul(qty))
            .ok_or_else(|| {
                invariant("continuous candle trade turnover overflow or invalid price")
            })?;
        if let Some(candle) = session
            .active_daily_candles
            .get(code)
            .filter(|candle| candle.volume > 0)
        {
            candle
                .volume
                .checked_add(qty)
                .ok_or_else(|| invariant("continuous candle volume overflow"))?;
            let stats = candle
                .trade_stats
                .as_ref()
                .ok_or_else(|| invariant("continuous active candle lacks trade statistics"))?;
            stats
                .turnover_cents
                .checked_add(gross)
                .ok_or_else(|| invariant("continuous candle turnover overflow"))?;
            stats
                .trade_count
                .checked_add(1)
                .ok_or_else(|| invariant("continuous candle trade count overflow"))?;
        }
    }
    session.update_active_daily_candle(code, price, qty);
    Ok(())
}

fn owned_event(event: Event, index: u64) -> OwnedEventFact {
    OwnedEventFact {
        key: EventStableKey::for_event(&event, index),
        event,
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::continuous_tick_finalizer".to_owned(),
    }
}
