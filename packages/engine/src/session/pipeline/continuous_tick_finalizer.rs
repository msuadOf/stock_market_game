//! 在 Continuous private candidate 上执行 stock_processing 收尾与唯一一次 ReceiptAggregation/Projection。

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
    stock_auction::auction_day_end::TradingDayEndTransition,
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
            .state
            .tick
            .checked_add(1)
            .ok_or_else(|| invariant("continuous tick overflow"))?;
        let ends_day = tick_after.is_multiple_of(session.state.setup.ticks_per_day);
        if ends_day && session.state.day == u32::MAX {
            return Err(invariant("continuous trading day overflow"));
        }
        let continuous_ticks = session
            .state
            .setup
            .ticks_per_day
            .checked_sub(session.state.setup.auction_ticks)
            .and_then(|ticks| ticks.checked_sub(session.state.setup.closing_auction_ticks))
            .ok_or_else(|| invariant("invalid continuous trading window"))?;
        let completed_ticks = session.state.tick % session.state.setup.ticks_per_day
            - session.state.setup.auction_ticks
            + 1;
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

/// `session` 是 Continuous 的可丢弃 candidate；stock_processing 在日终清簿前冻结行情与深度。
/// Settlement 使用原 tick 的 market minute，之后才推进时钟并按既定日界规则解锁 T+1。
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
    if session.state.next_receipt_base != session.state.envelope_ledger.next_receipt_index() {
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
        let prices =
            session.state.price_history.get_mut(code).ok_or_else(|| {
                finalize_error(invariant("continuous market has no price history"))
            })?;
        prices.push_back(last);
        while prices.len() > session.state.setup.history_len {
            prices.pop_front();
        }
        let minutes = session
            .state
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
        let day_start = u64::from(session.state.day) * u64::from(GAME_INTRADAY_MINUTES_PER_DAY);
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
                daily_candle: session.state.candle_book.active()[code].clone(),
                bids,
                asks,
            },
            0,
        ));
    }

    let transaction = apply_stock_execution_transaction_with_preceding_beliefs(
        &session.state.envelope_ledger,
        &session.state.accounts,
        &session.state.retail_experience,
        &session.state.belief_participants,
        &session.state.retail_projection_seen,
        finish.workers,
        SettlementApplicationContext::new(
            crate::experience::ExperienceMoment {
                civil_date: session.civil_date(),
                market_minute: session.current_market_minute(),
                trading_day: u64::from(session.state.day),
            },
            preceding_receipts,
            session.state.setup.t1_enabled,
        ),
    )
    .map_err(|error| {
        ContinuousTransactionError::SessionExecution(
            SessionExecutionTransactionError::StockExecution(error),
        )
    })?;
    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    session.state.envelope_ledger = transaction.ledger;
    session.state.next_receipt_base = session.state.envelope_ledger.next_receipt_index();
    session.state.accounts.extend(transaction.account_patch);
    session
        .state
        .retail_experience
        .extend(transaction.retail_patch);
    for (id, book) in transaction.belief_patch {
        let mut participant = session
            .state
            .belief_participants
            .remove(&id)
            .expect("prepared institutional belief participant is missing");
        *participant.belief_mut() = book;
        session.state.belief_participants.insert(id, participant);
    }
    session.state.retail_projection_seen = transaction.seen;
    for (code, stock) in transaction.stocks {
        session.state.markets.insert(code, stock.market);
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
    session.state.tick = boundary.tick_after;

    if boundary.ends_day {
        facts.extend(
            ContinuousDayEndLifecycleProjection::new(
                session,
                &transaction.receipts,
                day_end_event_base,
            )
            .apply()
            .map_err(finalize_error)?,
        );
        facts.extend(
            TradingDayEndTransition::new(session)
                .apply()
                .map_err(|error| {
                    finalize_error(invariant(&format!("continuous DayEnd failed: {error}")))
                })?,
        );
    }
    let collected = collect_events(facts, session.state.seq).map_err(finalize_error)?;
    session.state.seq = collected.next_seq;
    Ok(SessionExecutionTransactionOutput {
        events: collected.events,
        event_keys: collected.keys,
        receipts: transaction.receipts,
        settlement: transaction.settlement,
    })
}

/// 只投影 DayEnd release；Settlement 与共同日终转换由外层继续执行。
struct ContinuousDayEndLifecycleProjection<'a> {
    candidate: &'a mut GameSession,
    releases: Vec<&'a super::EnvelopeReceipt>,
    day_end_event_base: u64,
    facts: Vec<OwnedEventFact>,
    #[cfg(feature = "simulation-diagnostics")]
    day_end_causal_time: crate::diagnostics::causal::FactTime,
    #[cfg(feature = "simulation-diagnostics")]
    cleared_quote_codes: std::collections::BTreeSet<StockCode>,
}

impl<'a> ContinuousDayEndLifecycleProjection<'a> {
    fn new(
        session: &'a mut GameSession,
        receipts: &'a [super::EnvelopeReceipt],
        day_end_event_base: u64,
    ) -> Self {
        #[cfg(feature = "simulation-diagnostics")]
        let day_end_causal_time = {
            let mut time = session.causal_time();
            time.phase = TradingPhase::Continuous;
            time
        };
        let mut releases = receipts
            .iter()
            .filter(|receipt| matches!(receipt.local_key.source(), ReceiptSource::DayEnd(_)))
            .collect::<Vec<_>>();
        releases.sort_by(|left, right| left.envelope.cmp(&right.envelope));
        #[cfg(feature = "simulation-diagnostics")]
        let cleared_quote_codes = releases
            .iter()
            .map(|release| release.envelope.stock.clone())
            .collect::<std::collections::BTreeSet<_>>();

        Self {
            candidate: session,
            releases,
            day_end_event_base,
            facts: Vec::new(),
            #[cfg(feature = "simulation-diagnostics")]
            day_end_causal_time,
            #[cfg(feature = "simulation-diagnostics")]
            cleared_quote_codes,
        }
    }

    fn apply(mut self) -> Result<Vec<OwnedEventFact>, StepFatal> {
        for (ordinal, release) in std::mem::take(&mut self.releases).into_iter().enumerate() {
            self.apply_release(ordinal, release)?;
        }
        self.finish()
    }

    fn apply_release(
        &mut self,
        ordinal: usize,
        release: &super::EnvelopeReceipt,
    ) -> Result<(), StepFatal> {
        let key = &release.envelope;
        #[cfg(feature = "simulation-diagnostics")]
        self.candidate.causal_terminated_at(
            self.day_end_causal_time,
            (key.account, key.order, release.qty_before),
            &key.stock,
            crate::diagnostics::causal::Termination::DayEnd,
        );
        self.candidate
            .record_parent_order_canceled(key.account, &key.stock, key.order);
        self.candidate
            .remove_npc_order_lifecycle(key.account, &key.stock, key.order);
        if self
            .candidate
            .state
            .retail_experience
            .contains_key(&key.account)
        {
            self.candidate.state.last_retail_order_events.push(
                RetailOrderDiagnosticEvent::Canceled {
                    account: key.account,
                    code: key.stock.clone(),
                    order_id: key.order,
                    remaining_qty: release.qty_before,
                },
            );
        }
        let local_index = u64::try_from(ordinal)
            .ok()
            .and_then(|ordinal| self.day_end_event_base.checked_add(ordinal))
            .ok_or_else(|| invariant("continuous DayEnd event index overflow"))?;
        self.facts.push(owned_event(
            Event::OrderCanceled {
                seq: 0,
                account: key.account,
                code: key.stock.clone(),
                id: key.order,
                remaining_qty: release.qty_before,
            },
            local_index,
        ));

        Ok(())
    }

    fn finish(self) -> Result<Vec<OwnedEventFact>, StepFatal> {
        #[cfg(feature = "simulation-diagnostics")]
        for code in self.cleared_quote_codes {
            self.candidate
                .causal_snapshot_at(self.day_end_causal_time, &code);
        }
        let ended = self
            .candidate
            .state
            .parent_orders
            .values()
            .flat_map(|plans| plans.values())
            .filter(|parent| parent.filled_qty() < parent.target_qty())
            .filter_map(|parent| parent.linked_plan_id())
            .collect::<Vec<_>>();
        self.candidate
            .state
            .pending_plan_events
            .extend(ended.into_iter().map(|plan_id| PendingPlanEvent::DayEnded {
                plan_id,
                trading_day: u64::from(self.candidate.state.day),
            }));

        Ok(self.facts)
    }
}

/// 保持既有 candle reducer 的首笔成交与 open 语义；先检查算术，使失败返回 typed fatal。
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
            .state
            .candle_book
            .active()
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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use crate::session::pipeline::{Envelope, EnvelopeAudit, EnvelopeKey, FeeComponents};
    use crate::{AccountId, OrderId, Side};

    fn candidate() -> GameSession {
        let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
        setup.npcs.retail_count = 1;
        setup.npcs.inst_count = 0;
        GameSession::new(setup, 42).unwrap()
    }

    fn release(code: StockCode, account: AccountId, order: u64) -> super::super::EnvelopeReceipt {
        let envelope = Envelope::tick_start_existing(
            EnvelopeKey {
                account,
                stock: code,
                order: OrderId(order),
                side: Side::Sell,
            },
            Money::ZERO,
            100,
            EnvelopeAudit {
                limit: Money::from_cents(1_000),
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        );
        super::super::stock_auction::day_end_release_receipt(
            &envelope,
            u32::try_from(order).unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn empty_day_end_release_projection_has_no_order_outbox() {
        let mut session = candidate();
        let before = session.state.last_retail_order_events.clone();
        let facts = ContinuousDayEndLifecycleProjection::new(&mut session, &[], 10)
            .apply()
            .unwrap();
        assert!(facts.is_empty());
        assert_eq!(session.state.last_retail_order_events, before);
    }

    #[test]
    fn day_end_release_projection_orders_envelopes_and_keeps_partial_candidate_on_index_error() {
        let mut session = candidate();
        let code = session.state.markets.keys().next().unwrap().clone();
        let receipts = [
            release(code.clone(), AccountId(1), 11),
            release(code, AccountId(0), 10),
        ];
        let facts = ContinuousDayEndLifecycleProjection::new(&mut session, &receipts, 20)
            .apply()
            .unwrap();
        assert_eq!(
            facts.iter().map(|fact| &fact.event).collect::<Vec<_>>(),
            vec![
                &Event::OrderCanceled {
                    seq: 0,
                    account: AccountId(0),
                    code: receipts[1].envelope.stock.clone(),
                    id: OrderId(10),
                    remaining_qty: 100
                },
                &Event::OrderCanceled {
                    seq: 0,
                    account: AccountId(1),
                    code: receipts[0].envelope.stock.clone(),
                    id: OrderId(11),
                    remaining_qty: 100
                },
            ]
        );
        assert_ne!(facts[0].key, facts[1].key);
        assert_eq!(session.state.last_retail_order_events.len(), 1);

        let mut session = candidate();
        let error = ContinuousDayEndLifecycleProjection::new(&mut session, &receipts, u64::MAX)
            .apply()
            .unwrap_err();
        assert_eq!(error, invariant("continuous DayEnd event index overflow"));
        assert_eq!(
            session.state.last_retail_order_events.len(),
            1,
            "index 错误前已有的 candidate 生命周期写入保留，外层丢弃整个 candidate"
        );
    }
}
