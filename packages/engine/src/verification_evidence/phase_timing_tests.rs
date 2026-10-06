use super::*;

fn production_session(auction_ticks: u64) -> crate::GameSession {
    session_with_institutions(auction_ticks, 1)
}

fn session_with_institutions(auction_ticks: u64, inst_count: u32) -> crate::GameSession {
    use crate::{
        session::{
            FloatAllocation, NpcSetup, SecurityCategory, SessionSetup, StockExchange, StockSpec,
            SIMULATION_POLICY_ID,
        },
        GameConfig, HotParams, InstParams, Money, RetailParams, StockCode, StrategyParams,
    };

    crate::GameSession::new(
        SessionSetup {
            company_system: simple_company_fixture!(crate; ["600888"]),
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
                inst_count,
                hot_count: 0,
                retail_cash_median: Money::from_cents(10_000_000),
            },
            config: GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.0,
                    order_size_mean: 100,
                    chase_prob: 0.0,
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
            report_frequency: crate::information::ReportFrequency::Quarterly,
            float_allocation: FloatAllocation::random(),
            start_date: crate::CivilDate::from_ymd(2030, 1, 1).unwrap(),
            simulation_policy_id: SIMULATION_POLICY_ID.to_owned(),
            dividend_tax_mode: crate::company::cash_dividend_tax::CashDividendTaxMode::Exempt,
        },
        42,
    )
    .unwrap()
}

#[test]
fn public_timed_step_captures_one_committed_expiry_to_commit_record() {
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
    assert_eq!(
        value["schema"],
        serde_json::json!("escrow-committed-phase-timing")
    );
    assert_eq!(value["schema_version"], serde_json::json!(2));
    assert!(value["tick_before"].is_string());
    assert!(value["tick_after"].is_string());
    assert_eq!(
        value["records"][2]["phase"],
        "decision_and_coordinator_work"
    );
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

#[test]
fn empty_and_nonempty_ingress_both_record_account_validation() {
    for with_request in [false, true] {
        let mut session = session_with_institutions(0, 0);
        if with_request {
            session
                .enqueue_player_intent(
                    crate::AccountId(0),
                    crate::Intent::PlaceLimit {
                        code: crate::StockCode("600888".to_owned()),
                        side: crate::Side::Buy,
                        price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
                        qty: 100,
                    },
                )
                .unwrap();
        }
        let timed = session.step_with_phase_timing().unwrap();
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
            assert!(record.span_count() > 0);
            assert!(record.runnable_thread_sample().sample_count() >= 2);
        }
        assert_eq!(
            timed.events().iter().any(|event| matches!(
                event,
                crate::Event::OrderAccepted {
                    account: crate::AccountId(0),
                    remaining_qty: 100,
                    ..
                }
            )),
            with_request
        );
    }
}

#[test]
fn phase_timing_identity_keeps_rank_index_names_and_tick_mapping() {
    let tick_phases = [
        TickPhase::ExpiryShadow,
        TickPhase::SealAllocationSnapshot,
        TickPhase::DecisionShadow,
        TickPhase::AccountValidation,
        TickPhase::StockProcessing,
        TickPhase::ReceiptAggregation,
        TickPhase::SettlementShadow,
        TickPhase::DerivationAudit,
        TickPhase::PreCommitValidation,
        TickPhase::CommitTick,
    ];
    let names = [
        "expiry_shadow",
        "seal_allocation_snapshot",
        "decision_and_coordinator_work",
        "account_validation",
        "stock_processing",
        "receipt_aggregation",
        "settlement_shadow",
        "derivation_audit",
        "pre_commit_validation",
        "commit_tick",
    ];
    for (index, phase) in PhaseTimingPhase::ALL.into_iter().enumerate() {
        assert_eq!(phase.rank() as usize, index);
        assert_eq!(phase.index(), index);
        assert_eq!(phase.name(), names[index]);
        assert_eq!(PhaseTimingPhase::from(tick_phases[index]), phase);
    }
}

#[test]
fn phase_timing_sample_overflow_keeps_previous_bounds() {
    let mut accumulator = Accumulator {
        sample_count: u64::MAX,
        runnable_minimum: 2,
        runnable_maximum: 3,
        ..Accumulator::EMPTY
    };
    assert_eq!(accumulator.sample(1), Err("runnable_thread_sample_count"));
    assert_eq!(
        (
            accumulator.sample_count,
            accumulator.runnable_minimum,
            accumulator.runnable_maximum
        ),
        (u64::MAX, 2, 3)
    );
}

#[test]
fn phase_timing_ledger_preserves_overflow_order_and_partial_samples() {
    let phase = PhaseTimingPhase::ExpiryShadow;
    let mut ledger = PhaseTimingLedger::new();
    *ledger.accumulator_mut(phase) = Accumulator {
        wall_time_ns: u128::MAX,
        span_count: u64::MAX,
        ..Accumulator::EMPTY
    };
    assert_eq!(ledger.record_span(phase, 1), Some("span_count"));
    let value = ledger.accumulator(phase);
    assert_eq!(
        (value.wall_time_ns, value.span_count, value.sample_count),
        (u128::MAX, u64::MAX, 1)
    );
    ledger.accumulator_mut(phase).sample_count = u64::MAX;
    assert_eq!(
        ledger.record_span(phase, 1),
        Some("runnable_thread_sample_count")
    );
    assert_eq!(
        ledger.missing_precommit_phase(),
        Some(PhaseTimingPhase::SealAllocationSnapshot)
    );
}

#[test]
fn phase_timing_ledger_records_reject_missing_sample_after_precommit_spans() {
    let mut ledger = PhaseTimingLedger::new();
    for phase in PhaseTimingPhase::ALL.into_iter().take(9) {
        ledger.accumulator_mut(phase).span_count = 1;
    }
    assert_eq!(ledger.missing_precommit_phase(), None);
    assert_eq!(
        ledger.records().unwrap_err(),
        PhaseTimingCaptureError::IncompletePhase {
            phase: "expiry_shadow"
        }
    );
}
