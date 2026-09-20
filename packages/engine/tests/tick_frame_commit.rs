use engine::{
    CivilDate, FloatAllocation, GameConfig, HotParams, InstParams, Money, NpcSetup, RetailParams,
    SecurityCategory, SessionSetup, StockCode, StockExchange, StockSpec, StrategyParams,
};

use engine::session::protocol::ProtocolSession;

fn session() -> ProtocolSession {
    session_on("2030-01-02")
}

fn session_on(date: &str) -> ProtocolSession {
    ProtocolSession::new(
        SessionSetup {
            stocks: vec![StockSpec {
                code: StockCode("600001".into()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.1,
                tick: Money::from_cents(1),
                total_shares: 1_000_000,
                float_shares: 0,
            }],
            npcs: NpcSetup {
                retail_count: 0,
                inst_count: 0,
                hot_count: 0,
                retail_cash_median: Money::ZERO,
            },
            config: GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.0,
                    order_size_mean: 100,
                    chase_prob: 0.0,
                    tick_cents: 1,
                },
                inst: InstParams {
                    margin: 0.03,
                    order_size: 100,
                },
                hot: HotParams {
                    lookback: 2,
                    trend_threshold: 0.01,
                    order_size: 100,
                },
            },
            ticks_per_day: 20,
            auction_ticks: 6,
            closing_auction_ticks: 2,
            history_len: 20,
            t1_enabled: true,
            float_allocation: FloatAllocation::Random,
            start_date: CivilDate::from_iso(date).unwrap(),
            simulation_policy_id: engine::SIMULATION_POLICY_ID_V2.into(),
        },
        42,
    )
    .unwrap()
}

#[test]
fn frame_is_returned_after_committed_state_advances() {
    let mut game = session();
    let before = game.game().seq();
    let frame = game.step_frame().unwrap();
    assert_eq!(frame.tick, game.game().tick());
    assert_eq!(frame.tick, 1);
    assert_eq!(frame.seq_from, before);
    assert_eq!(frame.seq_to, game.game().seq());
    assert!(frame.validate().is_ok());
    assert_eq!(frame.timeseries_payload.auction_points.len(), 1);
    assert_eq!(
        serde_json::to_value(frame.timeseries_payload.markets).unwrap(),
        serde_json::to_value(game.game().snapshot().markets).unwrap()
    );
}

#[test]
fn quiet_preopen_ticks_are_not_dropped() {
    let mut game = session();
    for _ in 0..4 {
        game.step_frame().unwrap();
    }
    let before = game.game().seq();
    let frame = game.step_frame().unwrap();
    assert!(frame.events.is_empty());
    assert_eq!(frame.tick, 5);
    assert_eq!((frame.seq_from, frame.seq_to), (before, before));
    assert!(frame.validate().is_ok());
}

#[test]
fn civil_barrier_preserves_tick_and_complete_closed_day() {
    let mut game = session();
    for _ in 0..20 {
        game.step_frame().unwrap();
    }
    let seq = game.game().seq();
    let update = game.end_civil_day_update().unwrap();
    assert_eq!(update.tick, 20);
    assert_eq!(game.game().tick(), 20);
    assert_eq!(update.seq_from, seq);
    assert_eq!(update.seq_to, game.game().seq());
    assert!(update.seq_to > seq);
    assert_eq!(update.refresh.intraday.len(), 20);
    assert!(!update.refresh.snapshot.daily_candles.is_empty());
    assert_eq!(update.refresh.securities.len(), 1);
    assert!(update.validate().is_ok());
    assert!(update
        .kinds
        .contains(&engine::session::protocol::CivilUpdateKind::AfterClose));
    assert!(update
        .kinds
        .contains(&engine::session::protocol::CivilUpdateKind::BeforeOpen));
    let next = game.step_frame().unwrap();
    assert_eq!(next.tick, update.tick + 1);
    assert_eq!(next.seq_from, update.seq_to);
}

#[test]
fn premature_close_rejects_before_civil_mutation() {
    let mut game = session();
    for _ in 0..19 {
        game.step_frame().unwrap();
    }
    let before = game.game().business_state_hash().unwrap();
    assert!(game.end_civil_day_update().is_err());
    assert_eq!(before, game.game().business_state_hash().unwrap());
}

#[test]
fn caller_cannot_mutate_retained_authoritative_history() {
    let mut game = session();
    for _ in 0..20 {
        let mut frame = game.step_frame().unwrap();
        frame.timeseries_payload.markets.clear();
    }
    let update = game.end_civil_day_update().unwrap();
    assert!(update
        .refresh
        .intraday
        .iter()
        .all(|frame| frame.timeseries_payload.markets.len() == 1));
}

#[test]
fn civil_wire_validation_rejects_missing_history_and_wrong_date() {
    let mut game = session();
    for _ in 0..20 {
        game.step_frame().unwrap();
    }
    let original = game.end_civil_day_update().unwrap();
    let mut missing = original.clone();
    missing.refresh.intraday.clear();
    assert!(missing.validate().is_err());
    let mut wrong_date = original;
    wrong_date.civil_date = "2030-01-01".into();
    assert!(wrong_date.validate().is_err());
}

#[test]
fn pause_preferences_default_to_automatic_and_select_each_barrier() {
    use engine::session::protocol::{CivilUpdateKind, PausePreferences};
    let mut game = session();
    for _ in 0..20 {
        game.step_frame().unwrap();
    }
    let mut update = game.end_civil_day_update().unwrap();
    assert!(!PausePreferences::default().pauses(&update));
    let after = PausePreferences {
        pause_after_close: true,
        pause_before_open: false,
    };
    let before = PausePreferences {
        pause_after_close: false,
        pause_before_open: true,
    };
    update.kinds = vec![CivilUpdateKind::AfterClose];
    assert!(after.pauses(&update));
    assert!(!before.pauses(&update));
    update.kinds = vec![CivilUpdateKind::BeforeOpen];
    assert!(!after.pauses(&update));
    assert!(before.pauses(&update));
}

#[test]
fn deleting_after_close_and_history_is_rejected() {
    let mut game = session();
    for _ in 0..20 {
        game.step_frame().unwrap();
    }
    let mut update = game.end_civil_day_update().unwrap();
    update
        .kinds
        .retain(|kind| *kind != engine::session::protocol::CivilUpdateKind::AfterClose);
    update.refresh.intraday.clear();
    assert!(update.validate().is_err());
}

#[test]
fn completion_is_explicit_in_auction_projection() {
    let mut game = session();
    let mut completed = false;
    for _ in 0..20 {
        let frame = game.step_frame().unwrap();
        if frame
            .events
            .iter()
            .any(|event| matches!(event, engine::Event::AuctionCompleted { .. }))
        {
            let wire = serde_json::to_string(&frame.timeseries_payload).unwrap();
            assert!(wire.contains("Completion"));
            completed = true;
        }
    }
    assert!(completed);
}

#[test]
fn closed_weekend_boundaries_have_exact_kinds() {
    use engine::session::protocol::CivilUpdateKind;
    for (date, expected) in [
        ("2030-01-05", vec![CivilUpdateKind::CivilAdvance]),
        ("2030-01-06", vec![CivilUpdateKind::BeforeOpen]),
    ] {
        let mut game = session_on(date);
        let mut update = game.end_civil_day_update().unwrap();
        assert_eq!(update.kinds, expected);
        assert!(update.validate().is_ok());
        update.kinds.push(CivilUpdateKind::AfterClose);
        assert!(update.validate().is_err());
    }
}

#[test]
fn friday_close_has_no_before_open() {
    let mut game = session_on("2030-01-04");
    for _ in 0..20 {
        game.step_frame().unwrap();
    }
    let update = game.end_civil_day_update().unwrap();
    assert_eq!(
        update.kinds,
        vec![engine::session::protocol::CivilUpdateKind::AfterClose]
    );
    assert!(update.validate().is_ok());
}

#[test]
fn continuous_points_match_authoritative_price_tick() {
    let mut game = session();
    for _ in 0..7 {
        game.step_frame().unwrap();
    }
    let frame = game.step_frame().unwrap();
    for event in &frame.events {
        if let engine::Event::PriceTick {
            code,
            tick,
            last_price,
            daily_candle,
            bids,
            asks,
            ..
        } = event
        {
            let point = &frame.timeseries_payload.continuous_points[code];
            assert_eq!(
                (point.tick, point.last_price, point.cumulative_volume),
                (*tick, *last_price, daily_candle.volume)
            );
            assert_eq!((&point.bids, &point.asks), (bids, asks));
        }
    }
    assert_eq!(frame.timeseries_payload.continuous_points.len(), 1);
}
