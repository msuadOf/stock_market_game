use super::*;

fn production_session(auction_ticks: u64) -> crate::GameSession {
    use crate::{
        session::{
            FloatAllocation, NpcSetup, SecurityCategory, SessionSetup, StockExchange, StockSpec,
            SIMULATION_POLICY_ID_V2,
        },
        GameConfig, HotParams, InstParams, Money, RetailParams, StockCode, StrategyParams,
    };

    crate::GameSession::new(
        SessionSetup {
            stocks: vec![StockSpec {
                code: StockCode("600888".to_owned()),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1_000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: 10_000_000,
                float_shares: 0,
            }],
            npcs: NpcSetup {
                retail_count: 0,
                inst_count: 1,
                hot_count: 0,
                retail_cash_median: Money::from_cents(10_000_000),
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
                    margin: 0.05,
                    order_size: 100,
                },
                hot: HotParams {
                    lookback: 2,
                    trend_threshold: 0.01,
                    order_size: 100,
                },
            },
            ticks_per_day: 100,
            auction_ticks,
            closing_auction_ticks: 0,
            history_len: 10,
            t1_enabled: true,
            float_allocation: FloatAllocation::Random,
            start_date: crate::CivilDate::from_ymd(2030, 1, 1).unwrap(),
            simulation_policy_id: SIMULATION_POLICY_ID_V2.to_owned(),
        },
        42,
    )
    .unwrap()
}

#[test]
fn public_timed_step_captures_one_committed_p0_p9_record() {
    let mut session = production_session(0);
    let tick_before = session.tick();

    let timed = session.step_with_phase_timing().unwrap();

    assert_eq!(timed.timing().tick_before(), tick_before);
    assert_eq!(timed.timing().tick_after(), session.tick());
    assert_eq!(timed.timing().records().len(), PhaseTimingPhase::ALL.len());
    assert_eq!(
        timed
            .timing()
            .records()
            .iter()
            .map(PhaseTimingRecord::phase)
            .collect::<Vec<_>>(),
        PhaseTimingPhase::ALL
    );
    for record in timed.timing().records() {
        assert!(
            record.span_count() > 0,
            "{} had no measured span",
            record.phase().name()
        );
        assert!(record.runnable_thread_sample().sample_count() > 0);
        assert!(record.runnable_thread_sample().minimum() > 0);
        assert!(record.runnable_thread_sample().maximum() > 0);
    }
}

#[test]
fn opt_in_timing_does_not_change_events_save_or_business_hash() {
    let mut ordinary = production_session(0);
    let mut timed = production_session(0);

    let ordinary_events = ordinary.step().unwrap();
    let timed_result = timed.step_with_phase_timing().unwrap();

    assert_eq!(timed_result.events(), ordinary_events.as_slice());
    assert_eq!(
        timed.business_state_hash().unwrap(),
        ordinary.business_state_hash().unwrap()
    );
    assert_eq!(
        serde_json::to_vec(&timed.save().unwrap()).unwrap(),
        serde_json::to_vec(&ordinary.save().unwrap()).unwrap()
    );
}

#[test]
fn failed_step_returns_no_committed_timing_evidence() {
    let mut session = production_session(0);
    let fatal = crate::session::StepFatal::InvariantViolation {
        description: "phase timing failure fixture".to_owned(),
        location: "verification_evidence::phase_timing_tests".to_owned(),
    };
    session.inject_post_shadow_failure(fatal.clone());

    let error = session.step_with_phase_timing().unwrap_err();

    assert_eq!(error, PhaseTimingCaptureError::Step(fatal));
}

#[test]
fn capture_rejects_nested_or_non_step_use_before_committed_evidence_can_exist() {
    let mut session = production_session(0);
    let nested = std::cell::RefCell::new(None);
    let error = with_phase_timing_capture_for_test(|| {
        nested.replace(Some(session.step_with_phase_timing().unwrap_err()));
        Err::<(), _>("stop after observing nested capture".to_owned())
    })
    .unwrap_err();

    assert_eq!(error, PhaseTimingCaptureError::NoCommittedTick);
    assert_eq!(
        nested.into_inner(),
        Some(PhaseTimingCaptureError::CaptureAlreadyActive)
    );

    let error = with_phase_timing_capture_for_test(|| Ok::<_, String>(())).unwrap_err();
    assert_eq!(error, PhaseTimingCaptureError::NoCommittedTick);
}

#[test]
fn auction_and_pre_open_dispatchers_each_emit_complete_committed_timing() {
    let mut session = production_session(3);
    assert_eq!(session.phase(), crate::TradingPhase::CallAuction);
    let auction = session.step_with_phase_timing().unwrap();
    assert_eq!(auction.timing().records().len(), 10);

    session.step().unwrap();
    assert_eq!(session.phase(), crate::TradingPhase::PreOpen);
    let pre_open = session.step_with_phase_timing().unwrap();
    assert_eq!(pre_open.timing().records().len(), 10);
}

#[test]
fn evidence_serializes_all_measurements_as_decimal_strings() {
    let mut session = production_session(0);
    let timed = session.step_with_phase_timing().unwrap();
    let value = serde_json::to_value(timed.timing()).unwrap();

    assert_eq!(
        value["schema"],
        serde_json::json!(CommittedPhaseTiming::SCHEMA)
    );
    assert!(value["tick_before"].is_string());
    assert!(value["tick_after"].is_string());
    for record in value["records"].as_array().unwrap() {
        assert!(record["wall_time_ns"].is_string());
        assert!(record["span_count"].is_string());
        assert!(record["runnable_threads"]["sample_count"].is_string());
        assert!(record["runnable_threads"]["minimum"].is_string());
        assert!(record["runnable_threads"]["maximum"].is_string());
    }
}

#[test]
fn runnable_thread_samples_follow_the_real_rayon_registry() {
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build()
        .unwrap();
    let timing = pool.install(|| {
        production_session(0)
            .step_with_phase_timing()
            .unwrap()
            .timing
    });

    for record in timing.records() {
        assert_eq!(record.runnable_thread_sample().minimum(), 2);
        assert_eq!(record.runnable_thread_sample().maximum(), 2);
    }
}
