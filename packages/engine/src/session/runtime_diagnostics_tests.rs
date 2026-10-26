//! D1 失败原子性：真实 runtime 事实须等最外成功交付才进入 collector。

use super::*;
use crate::diagnostics::causal::CausalFactKind;

fn game() -> GameSession {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.ticks_per_day = 30;
    GameSession::new(setup, 7).unwrap()
}

fn enqueue(game: &mut GameSession) {
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: StockId(0),
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(995)),
            qty: 100,
        },
    )
    .unwrap();
}

fn driver(game: &mut GameSession) -> crate::runtime::drive::RuntimeDriver {
    let intents = game.take_runtime_player_intents();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(game).unwrap();
    let upper = driver.world_public_views().iter().map(|view| view.daily_upper_limit).collect::<Vec<_>>();
    driver.inject_player_intents(&intents, &upper).unwrap();
    driver
}

fn successful_submission(game: &mut GameSession) {
    game.step().unwrap();
    assert_eq!(
        game.causal_facts()
            .iter()
            .filter(|fact| matches!(fact.kind, CausalFactKind::Submitted(_)))
            .count(),
        1
    );
    assert_eq!(game.causal_facts().len(), 3);
}

#[test]
fn d1_callback_failure_discards_delta_then_retry_publishes_once() {
    let mut game = game();
    enqueue(&mut game);
    let before = game.business_state_hash().unwrap();
    let trace_before = serde_json::to_value(&game.state.npc_decision_traces).unwrap();
    let mut called = false;
    let rejected = game.step_frame_and_commit_via_runtime(|_, events| {
        called = true;
        assert!(events
            .iter()
            .any(|fact| matches!(fact.event, Event::OrderAccepted { .. })));
        Err::<(), _>(StepFatal::InvariantViolation {
            location: "d1_callback_failure".to_owned(),
            description: "拒绝候选".to_owned(),
        })
    });
    assert!(called);
    assert!(rejected.is_err());
    assert!(game.causal_facts().is_empty());
    assert_eq!(
        trace_before,
        serde_json::to_value(&game.state.npc_decision_traces).unwrap()
    );
    assert_eq!(before, game.business_state_hash().unwrap());
    successful_submission(&mut game);
    game.step().unwrap();
    assert_eq!(game.causal_facts().len(), 3);
}

#[test]
fn d1_install_failure_keeps_prior_facts_and_checkpoint_retry_has_no_duplicate() {
    let mut game = game();
    enqueue(&mut game);
    successful_submission(&mut game);
    enqueue(&mut game);
    let saved = game.clone_for_tick_shadow().unwrap();
    let before = serde_json::to_value(game.causal_facts()).unwrap();
    let mut driver = driver(&mut game);
    let before_hash = game.business_state_hash().unwrap();
    game.inject_runtime_install_failure();
    assert!(game.runtime_step_with_install(&mut driver).is_err());
    assert_eq!(serde_json::to_value(game.causal_facts()).unwrap(), before);
    assert_eq!(game.business_state_hash().unwrap(), before_hash);
    game = saved;
    game.step().unwrap();
    assert_eq!(game.causal_facts().len(), 6);
}

#[test]
fn d1_world_validator_abort_keeps_facts_and_successful_retry_publishes_once() {
    let mut game = game();
    enqueue(&mut game);
    let checkpoint = game.clone_for_tick_shadow().unwrap();
    let mut driver = driver(&mut game);
    let before = game.business_state_hash().unwrap();
    driver.set_world_validator_for_test(Box::new(|_| Err("D1 validator拒绝".to_owned())));
    assert!(game.runtime_step_with_install(&mut driver).is_err());
    assert!(game.causal_facts().is_empty());
    assert_eq!(before, game.business_state_hash().unwrap());
    game = checkpoint;
    successful_submission(&mut game);
}

#[test]
fn d1_seal_failure_keeps_facts_and_retry_publishes_once() {
    let mut game = game();
    enqueue(&mut game);
    let checkpoint = game.clone_for_tick_shadow().unwrap();
    let mut driver = driver(&mut game);
    let before = game.business_state_hash().unwrap();
    driver.fail_seal_once_for_test();
    assert!(game.runtime_step_with_install(&mut driver).is_err());
    assert!(game.causal_facts().is_empty());
    assert_eq!(before, game.business_state_hash().unwrap());
    game = checkpoint;
    successful_submission(&mut game);
}

#[test]
fn d1_unresolved_source_reports_error_before_plain_step_install_and_rolls_back() {
    let mut game = game();
    enqueue(&mut game);
    let checkpoint = game.clone_for_tick_shadow().unwrap();
    let mut driver = driver(&mut game);
    let before_hash = game.business_state_hash().unwrap();
    driver.unresolved_causal_source_for_test();
    let error = game.runtime_step_with_install(&mut driver).unwrap_err();
    assert!(matches!(error, StepFatal::InvariantViolation { ref location, ref description }
        if location == "runtime::diagnostics::prepare" && description.contains("NonPlayerUnresolved")));
    assert!(game.causal_facts().is_empty());
    assert_eq!(game.business_state_hash().unwrap(), before_hash);
    assert!(game.runtime_discard_driver_flag);
    game = checkpoint;
    successful_submission(&mut game);
}

#[test]
fn d1_unresolved_source_at_frame_entry_discards_driver_before_callback() {
    let mut game = game();
    enqueue(&mut game);
    let before_hash = game.business_state_hash().unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    driver.unresolved_causal_source_for_test();
    game.runtime_driver = Some(std::sync::Mutex::new(driver));
    let mut callback = false;
    let result = game.step_frame_and_commit_via_runtime(|_, _| {
        callback = true;
        Ok(())
    });
    assert!(result.is_err());
    assert!(!callback);
    assert!(game.runtime_driver.is_none());
    assert!(game.causal_facts().is_empty());
    assert_eq!(game.business_state_hash().unwrap(), before_hash);
}

#[test]
fn d1_success_consumes_sideband_and_next_tick_cannot_repeat_it() {
    let mut game = game();
    enqueue(&mut game);
    let mut driver = driver(&mut game);
    game.runtime_step_with_install(&mut driver).unwrap();
    assert_eq!(game.causal_facts().len(), 3);
    assert!(driver.world_report_log().iter().all(|record| record.desk_causal.is_empty()));
    game.runtime_step_with_install(&mut driver).unwrap();
    assert_eq!(game.causal_facts().len(), 3);
    assert!(driver.world_report_log().iter().all(|record| record.desk_causal.is_empty()));
}

#[test]
fn d1_real_same_tick_fill_is_explicitly_unsupported_and_rolls_back_both_legs() {
    let mut game = game();
    let seller = AccountId(99);
    let mut account = crate::Account::new(seller, crate::account::AccountKind::Player, Money::from_cents(1_000_000));
    account.fixture_set_positions(std::collections::BTreeMap::from([(
        StockId(0), crate::account::Position::from_restored_parts(100, 0, 100_000, 0),
    )]));
    game.state.accounts.insert(seller, account);
    game.enqueue_player_intent(seller, Intent::PlaceLimit {
        code: StockId(0), side: crate::Side::Sell,
        price: crate::LimitPrice::Fixed(Money::from_cents(995)), qty: 100,
    }).unwrap();
    enqueue(&mut game);
    let before = game.business_state_hash().unwrap();
    let outcome = game.step();
    assert!(matches!(outcome, Err(StepFatal::InvariantViolation { ref location, ref description })
        if location == "runtime::diagnostics::prepare" && description.contains("Filled/Execution")),
        "真实成交必须因未接Filled/Execution显式失败，不能留下假open：{outcome:?}");
    assert!(game.causal_facts().is_empty());
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn d1_each_manual_drain_entry_rejects_existing_resting_before_any_mutation() {
    for entry in 0..3 {
        let mut game = game();
        enqueue(&mut game);
        let mut driver = driver(&mut game);
        game.runtime_step_with_install(&mut driver).unwrap();
        let before = game.business_state_hash().unwrap();
        let before_facts = serde_json::to_value(game.causal_facts()).unwrap();
        let error = match entry {
            0 => driver.day_end(&mut game).unwrap_err(),
            1 => driver.day_end_with_runtime_market_day(&mut game).unwrap_err(),
            _ => driver.world_day_end_for_step(&mut game).unwrap_err(),
        };
        assert!(error.contains("causal DayEnd"), "真实余单必须在Drain前因诊断能力缺口拒绝：{error}");
        assert_eq!(game.business_state_hash().unwrap(), before);
        assert_eq!(serde_json::to_value(game.causal_facts()).unwrap(), before_facts);
        assert_eq!(driver.collect_resting_facts().unwrap()[0].len(), 1);
    }
}

#[test]
fn d1_manual_drain_after_checkpoint_driver_rebuild_checks_real_seeded_orders() {
    let mut game = game();
    enqueue(&mut game);
    successful_submission(&mut game);
    let mut copied = game.clone_for_tick_shadow().unwrap();
    let mut rebuilt = crate::runtime::drive::RuntimeDriver::from_session(&mut copied).unwrap();
    assert!(rebuilt.world_report_log().is_empty(), "重建的发布v0没有旧逐拍记录");
    let before = copied.business_state_hash().unwrap();
    let error = rebuilt.world_day_end_for_step(&mut copied).unwrap_err();
    assert!(error.contains("causal DayEnd"));
    assert_eq!(copied.business_state_hash().unwrap(), before);
    assert_eq!(copied.causal_facts().len(), 3);
}
