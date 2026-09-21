use super::b1_continuous_transaction::prepare_b1_continuous_tick;
use crate::{AccountId, Event, GameSession, Intent, Money, Side, TradingPhase};

fn session(ticks_per_day: u64, closing_ticks: u64) -> GameSession {
    let mut setup = crate::session::npc_working_quote_tests::two_stock_quote_setup();
    setup.npcs.inst_count = 0;
    setup.ticks_per_day = ticks_per_day;
    setup.closing_auction_ticks = closing_ticks;
    setup.history_len = 2;
    GameSession::new(setup, 42).unwrap()
}

#[test]
fn b1_empty_tick_records_each_stock_once_and_restores_at_the_commit_boundary() {
    let mut game = session(481, 1);
    for tick in 1..=4 {
        let result = prepare_b1_continuous_tick(&mut game).unwrap().commit();
        assert_eq!(game.tick(), tick);
        assert_eq!(result.commit.tick.events, result.output.events);
        assert_eq!(result.output.events.len(), 2);
        for (index, (code, market)) in game.markets.iter().enumerate() {
            assert!(matches!(&result.output.events[index], Event::PriceTick {
                seq, tick: event_tick, code: event_code, last_price, daily_candle, bids, asks,
            } if *seq == (tick - 1) * 2 + index as u64 + 1
                && *event_tick == tick && event_code == code && *last_price == market.last_price()
                && daily_candle == &game.active_daily_candles[code]
                && bids.is_empty() && asks.is_empty()));
            assert_eq!(game.price_history[code].len(), tick.min(2) as usize);
            assert_eq!(game.market_minute_closes[code].len(), (tick / 2) as usize);
        }
        let saved = game.save().unwrap();
        let restored = GameSession::restore(&saved).unwrap();
        assert_eq!(
            serde_json::to_value(restored.save().unwrap()).unwrap(),
            serde_json::to_value(saved).unwrap()
        );
    }
}

#[test]
fn b1_last_continuous_tick_keeps_its_price_point_before_closing_auction() {
    let mut game = session(4, 1);
    for _ in 0..3 {
        prepare_b1_continuous_tick(&mut game).unwrap().commit();
    }
    assert_eq!(game.tick(), 3);
    assert_eq!(game.day(), 0);
    assert_eq!(game.phase(), TradingPhase::ClosingAuction);
    assert!(game
        .market_minute_closes
        .values()
        .all(|history| history.len() == 240));
    GameSession::restore(&game.save().unwrap()).unwrap();
}

#[test]
fn b1_no_closing_auction_finishes_day_and_releases_new_orders_once() {
    let mut game = session(1, 0);
    let code = game.markets.keys().next().unwrap().clone();
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: Money::from_cents(990),
            qty: 100,
        },
    )
    .unwrap();
    let result = prepare_b1_continuous_tick(&mut game).unwrap().commit();
    assert_eq!(game.tick(), 1);
    assert_eq!(game.day(), 1);
    assert_eq!(result.output.receipts.len(), 1);
    assert!(matches!(
        result.output.receipts[0].local_key.source(),
        super::ReceiptSource::DayEnd(0)
    ));
    assert_eq!(result.output.p6.settlement.applied_receipts, 0);
    assert!(result.output.events.iter().any(|event| matches!(event,
        Event::PriceTick { code: actual, bids, .. }
        if actual == &code && bids == &vec![(Money::from_cents(990), 100)])));
    assert!(result.output.events.iter().any(|event| matches!(event,
        Event::OrderCanceled { code: actual, remaining_qty: 100, .. } if actual == &code)));
    assert_eq!(
        result
            .output
            .events
            .iter()
            .filter(|event| matches!(event, Event::DayBoundary { .. }))
            .count(),
        1
    );
    assert!(game.active_daily_candles.is_empty());
    assert!(game.market_minute_closes.values().all(Vec::is_empty));
    assert!(game
        .markets
        .values()
        .all(|market| market.resting_orders().is_empty()));
    assert_eq!(game.envelope_ledger.iter().count(), 0);
    let saved = game.save().unwrap();
    assert_eq!(
        serde_json::to_value(GameSession::restore(&saved).unwrap().save().unwrap()).unwrap(),
        serde_json::to_value(saved).unwrap()
    );
}

fn trade_session(ticks_per_day: u64) -> (GameSession, crate::StockCode) {
    let mut game = session(ticks_per_day, 0);
    let code = game.markets.keys().next().unwrap().clone();
    // Existing player inventory avoids adding an account absent from the save setup.
    game.accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), 400, Money::from_cents(1_000))
        .unwrap();
    for price in [1_010, 1_020] {
        for side in [Side::Sell, Side::Buy] {
            game.enqueue_player_intent(
                AccountId(0),
                Intent::PlaceLimit {
                    code: code.clone(),
                    side,
                    price: Money::from_cents(price),
                    qty: 100,
                },
            )
            .unwrap();
        }
    }
    (game, code)
}

#[test]
fn b1_multi_round_trades_record_first_real_open_and_final_depth_once() {
    let (mut game, code) = trade_session(4);
    game.update_active_daily_candle(&code, Money::from_cents(1_000), 0);
    let result = prepare_b1_continuous_tick(&mut game).unwrap().commit();
    let candle = &game.active_daily_candles[&code];
    assert_eq!(
        (candle.open, candle.low),
        (Money::from_cents(1_010), Money::from_cents(1_010))
    );
    assert_eq!(
        (candle.close, candle.high),
        (Money::from_cents(1_020), Money::from_cents(1_020))
    );
    assert_eq!(candle.volume, 200);
    let stats = candle.trade_stats.as_ref().unwrap();
    assert_eq!(stats.trade_count, 2);
    assert_eq!(stats.turnover_cents, 203_000);
    assert_eq!(game.accounts[&AccountId(0)].positions[&code].t1_locked, 200);
    assert!(result.output.events.iter().any(|event| matches!(event,
        Event::PriceTick { code: actual, last_price, bids, asks, daily_candle, .. }
        if actual == &code && *last_price == Money::from_cents(1_020)
            && bids.is_empty() && asks.is_empty() && daily_candle == candle)));
    assert_eq!(game.market_minute_closes[&code].len(), 60);
    assert!(game.market_minute_closes[&code]
        .iter()
        .enumerate()
        .all(
            |(minute, point)| point.absolute_trading_minute == minute as u64
                && point.close == Money::from_cents(1_020)
        ));

    let saved = game.save().unwrap();
    let mut restored = GameSession::restore(&saved).unwrap();
    let expected = prepare_b1_continuous_tick(&mut game).unwrap().commit();
    let actual = prepare_b1_continuous_tick(&mut restored).unwrap().commit();
    assert_eq!(actual.output.events, expected.output.events);
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        serde_json::to_value(game.save().unwrap()).unwrap()
    );
}

#[test]
fn b1_final_tick_settles_before_t1_unlock_and_commits_the_trade_candle_once() {
    let (mut game, code) = trade_session(1);
    let result = prepare_b1_continuous_tick(&mut game).unwrap().commit();
    assert_eq!(game.accounts[&AccountId(0)].positions[&code].t1_locked, 0);
    assert_eq!(game.accounts[&AccountId(0)].sellable_qty(&code), 400);
    let candle = game.daily_candles[&code].last().unwrap();
    assert_eq!(candle.volume, 200);
    assert_eq!(game.markets[&code].last_close(), Money::from_cents(1_020));
    assert_eq!(result.output.events.iter().filter(|event| matches!(event,
        Event::PriceTick { code: actual, daily_candle, .. } if actual == &code && daily_candle == candle)).count(), 1);
    assert!(result.output.events.iter().any(|event| matches!(event,
        Event::DayBoundary { day: 1, closed_daily_candles, .. } if &closed_daily_candles[&code] == candle)));
    GameSession::restore(&game.save().unwrap()).unwrap();
}

#[test]
fn b1_late_sequence_failure_discards_prices_candles_settlement_and_outbox() {
    let (mut game, _) = trade_session(1);
    game.seq = u64::MAX - 1;
    let authority_before = (
        game.business_state_hash().unwrap(),
        game.session_state_hash().unwrap(),
    );
    let mut plan = super::plan_tick(super::PhaseInput { session: &game }).unwrap();
    let shadow_before = plan
        .state
        .execute(|candidate| {
            Ok((
                candidate.business_state_hash()?,
                candidate.session_state_hash()?,
            ))
        })
        .unwrap();
    let error =
        super::b1_continuous_transaction::apply_tick_shadow_b1_continuous_transaction(&mut plan)
            .err()
            .expect("P7 sequence exhaustion must fail after the private tail");
    assert!(
        matches!(error.into_fatal(), super::StepFatal::InvariantViolation { location, description }
        if location == "pipeline::p7_events::collect_events" && description.contains("sequence overflow"))
    );
    assert!(plan.event_outbox.is_empty());
    assert!(plan.receipt_keys.is_empty());
    assert_eq!(
        plan.state
            .execute(|candidate| Ok((
                candidate.business_state_hash()?,
                candidate.session_state_hash()?
            )))
            .unwrap(),
        shadow_before
    );
    assert_eq!(
        (
            game.business_state_hash().unwrap(),
            game.session_state_hash().unwrap()
        ),
        authority_before
    );
}

#[test]
fn b1_tick_and_day_overflow_are_typed_and_atomic() {
    for overflow_tick in [true, false] {
        let mut game = session(1, 0);
        if overflow_tick {
            game.tick = u64::MAX;
        } else {
            game.day = u32::MAX;
        }
        let before = (
            game.business_state_hash().unwrap(),
            game.session_state_hash().unwrap(),
        );
        let error = prepare_b1_continuous_tick(&mut game)
            .err()
            .expect("clock overflow must fail");
        assert!(
            matches!(error.into_fatal(), super::StepFatal::InvariantViolation { location, description }
            if location == "pipeline::b1_tick_finalizer" && description.contains("overflow"))
        );
        assert_eq!(
            (
                game.business_state_hash().unwrap(),
                game.session_state_hash().unwrap()
            ),
            before
        );
    }
}

#[test]
fn b1_candle_counter_overflow_returns_fatal_without_committing_the_tick() {
    for counter in ["volume", "turnover", "count"] {
        let (mut game, code) = trade_session(4);
        game.update_active_daily_candle(&code, Money::from_cents(1_000), 100);
        let before = game.business_state_hash().unwrap();
        let mut plan = super::plan_tick(super::PhaseInput { session: &game }).unwrap();
        // Inject only into the private candidate: MAX volume/count are deliberately outside
        // the save protocol's JS-safe range, so P8 authority hashing must see healthy data.
        let injected = plan
            .state
            .execute(|candidate| {
                let candle = candidate.active_daily_candles.get_mut(&code).unwrap();
                match counter {
                    "volume" => candle.volume = u64::MAX,
                    "turnover" => candle.trade_stats.as_mut().unwrap().turnover_cents = u64::MAX,
                    "count" => candle.trade_stats.as_mut().unwrap().trade_count = u64::MAX,
                    _ => unreachable!(),
                }
                Ok(candle.clone())
            })
            .unwrap();
        let error = super::b1_continuous_transaction::apply_tick_shadow_b1_continuous_transaction(
            &mut plan,
        )
        .err()
        .expect("candle overflow must fail");
        assert!(
            matches!(error.into_fatal(), super::StepFatal::InvariantViolation { location, description }
            if location == "pipeline::b1_tick_finalizer" && description.contains("overflow"))
        );
        assert_eq!(game.business_state_hash().unwrap(), before);
        assert!(plan.event_outbox.is_empty());
        plan.state
            .execute(|candidate| {
                assert_eq!(candidate.active_daily_candles[&code], injected);
                assert_eq!(candidate.tick, 0);
                assert_eq!(candidate.pending_player.len(), 4);
                Ok(())
            })
            .unwrap();
    }
}

#[test]
fn b1_dropping_prepared_tick_keeps_the_previous_save_quiescent() {
    let mut game = session(4, 0);
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    drop(prepare_b1_continuous_tick(&mut game).unwrap());
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
    assert_eq!(game.tick(), 0);
}

#[test]
fn b1_partial_fill_then_day_end_keeps_cross_source_receipts_and_cancellation_quantity() {
    for (maker, taker) in [(Side::Sell, Side::Buy), (Side::Buy, Side::Sell)] {
        partial_fill_then_day_end(maker, taker);
    }
}

fn partial_fill_then_day_end(maker: Side, taker: Side) {
    let mut game = session(2, 0);
    let code = game.markets.keys().next().unwrap().clone();
    game.accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .grant_position(code.clone(), 200, Money::from_cents(1_000))
        .unwrap();
    for (side, qty) in [(maker, 200), (taker, 100)] {
        game.enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side,
                price: Money::from_cents(1_000),
                qty,
            },
        )
        .unwrap();
        if side == maker {
            prepare_b1_continuous_tick(&mut game).unwrap().commit();
        }
    }
    let saved = game.save().unwrap();
    let mut restored = GameSession::restore(&saved).unwrap();
    let result = prepare_b1_continuous_tick(&mut game).unwrap().commit();
    let replay = prepare_b1_continuous_tick(&mut restored).unwrap().commit();
    assert_eq!(result.output.events, replay.output.events);
    assert_eq!(
        format!("{:?}", result.output.receipts),
        format!("{:?}", replay.output.receipts)
    );
    assert_eq!(result.output.receipts.len(), 3);
    assert_eq!(result.output.p6.settlement.applied_receipts, 2);
    let released = result
        .output
        .receipts
        .iter()
        .find(|receipt| matches!(receipt.local_key.source(), super::ReceiptSource::DayEnd(_)))
        .unwrap();
    assert_eq!((released.qty_before, released.qty_after), (100, 100));
    assert_eq!(released.delta.live_after, super::ResVec::ZERO);
    let fill = result
        .output
        .receipts
        .iter()
        .find(|receipt| {
            receipt.envelope == released.envelope && receipt.kind == super::ReceiptKind::Fill
        })
        .unwrap();
    assert_eq!(fill.qty_after, released.qty_before);
    assert_eq!(fill.delta.live_after, released.delta.released);
    assert_eq!(
        released.delta.released.shares,
        if maker == Side::Sell { 100 } else { 0 }
    );
    assert!(result.output.events.iter().any(|event| matches!(
        event,
        Event::OrderCanceled {
            remaining_qty: 100,
            ..
        }
    )));
    assert!(result.output.events.iter().any(|event| matches!(event,
        Event::PriceTick { code: actual, asks, bids, .. }
        if actual == &code
            && (if maker == Side::Sell { asks } else { bids }) == &vec![(Money::from_cents(1_000), 100)])));
    assert_eq!(game.day(), 1);
    assert_eq!(game.envelope_ledger.iter().count(), 0);
    assert!(game.markets[&code].resting_orders().is_empty());
    assert_eq!(game.accounts[&AccountId(0)].positions[&code].t1_locked, 0);
    assert_eq!(game.daily_candles[&code].last().unwrap().volume, 100);
    assert_eq!(
        serde_json::to_value(game.save().unwrap()).unwrap(),
        serde_json::to_value(restored.save().unwrap()).unwrap()
    );
}

#[test]
fn b1_fatal_conversion_preserves_nested_p5_and_p6_identity() {
    use super::{
        b1_continuous_transaction::B1ContinuousTransactionError as B1,
        decision_snapshot_capture::DecisionSnapshotCaptureError as Snapshot,
        npc_p2_p7_transaction::NpcP2P7TransactionError as Npc,
        npc_p2_projection::NpcP2ProjectionError as Projection,
        p4_p5_p6_transaction::P4P5P6TransactionError as P4P6,
        p4_p7_session_transaction::P4P7SessionTransactionError as P4P7,
        p6_transaction::P6TransactionError as P6,
    };
    let mut game = session(4, 0);
    let expected = game.business_state_hash().unwrap();
    game.seq = 1;
    let observed = game.business_state_hash().unwrap();
    for fatal in [
        super::StepFatal::InvariantViolation {
            description: "preserve exact source".to_owned(),
            location: "test::source".to_owned(),
        },
        super::StepFatal::Internal { expected, observed },
    ] {
        for error in [
            B1::Preparation(fatal.clone()),
            B1::Finalization(fatal.clone()),
            B1::Npc(Npc::Preparation(fatal.clone())),
            B1::Npc(Npc::Snapshot(Snapshot::ShadowClone(fatal.clone()))),
            B1::Npc(Npc::Projection(Projection::ShadowClone(fatal.clone()))),
            B1::Npc(Npc::Projection(Projection::ResourceSnapshot {
                account: AccountId(0),
                source: fatal.clone(),
            })),
            B1::P4P7(P4P7::P4P6(P4P6::P5(fatal.clone()))),
            B1::P4P7(P4P7::P4P6(P4P6::P6(P6::Settlement(fatal.clone())))),
        ] {
            assert_eq!(error.into_fatal(), fatal);
        }
    }
}
