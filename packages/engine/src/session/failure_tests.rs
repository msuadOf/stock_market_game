use super::*;

#[test]
fn skeleton_failure_preserves_business_and_poison_context() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let business = game.business_state_hash().unwrap();
    let session = game.session_state_hash().unwrap();
    let fatal = StepFatal::InvariantViolation {
        description: "injected skeleton failure".to_owned(),
        location: "step.pre_mutation".to_owned(),
    };
    game.inject_step_failure(fatal.clone());
    assert_eq!(game.session_state_hash().unwrap(), session);
    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.business_state_hash().unwrap(), business);
    assert_ne!(game.session_state_hash().unwrap(), session);
    assert_eq!(game.poison_reason(), Some(&fatal));
    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.save().unwrap_err(), fatal);
    assert!(matches!(game.end_civil_day(), Err(SessionError::Step(error)) if error == fatal));
    let poisoned = game.poison.take();
    assert_eq!(game.session_state_hash().unwrap(), session);
    game.poison = poisoned;
    println!(
        "poison QA: exact Err; business unchanged; session changes only by poison; repeated step/save reject"
    );
}

#[test]
fn public_step_rejects_and_poisons_a_missing_npc_strategy() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.fixture_account_mut(&AccountId(1))
        .unwrap()
        .fixture_set_strategy(None);
    let business = game.business_state_hash().unwrap();

    let fatal = game.step().unwrap_err();

    assert_eq!(
        fatal,
        StepFatal::InvariantViolation {
            description: "non-player account has no authoritative strategy".to_owned(),
            location: "GameSession::step".to_owned(),
        }
    );
    assert_eq!(game.business_state_hash().unwrap(), business);
    assert_eq!(game.poison_reason(), Some(&fatal));
}

#[test]
fn injected_failure_runs_before_missing_npc_strategy_validation() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.fixture_account_mut(&AccountId(1))
        .unwrap()
        .fixture_set_strategy(None);
    let injected = StepFatal::InvariantViolation {
        description: "injected before malformed strategy validation".to_owned(),
        location: "failure_tests::missing_strategy_hook_order".to_owned(),
    };
    game.inject_step_failure(injected.clone());

    assert_eq!(game.step().unwrap_err(), injected);
    assert_eq!(game.poison_reason(), Some(&injected));
}

#[test]
fn business_hash_detects_pending_facts_counters_and_strategy_state() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let before = game.business_state_hash().unwrap();
    game.state.next_order_id += 1;
    assert_ne!(game.business_state_hash().unwrap(), before);
    game.state.next_order_id -= 1;
    game.state.next_receipt_base += 1;
    assert_ne!(game.business_state_hash().unwrap(), before);
    game.state.next_receipt_base -= 1;
    let plan_id = game
        .state
        .plans
        .create(crate::plans::PlanOpen {
            account: AccountId(1),
            code: StockId(0),
            direction: crate::Side::Buy,
            target: crate::plans::PlanTarget::ShareCount(100),
            opinion: crate::plans::PlanOpinion {
                signal_score_bp: 3_000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: crate::plans::Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    // Astra 终审 P2a：execution 与 strategy 各自建立"变更前"独立基线——
    // 旧写法在新增计划后仍用开局基线断言 strategy 差异，计划本身即让断言通过，
    // 检错力被稀释；现改为逐变更独立对照，恢复各自独立的检错能力。
    let with_plan = game.business_state_hash().unwrap();
    game.state
        .plans
        .install_execution(plan_id, 100, crate::Money::from_cents(1_000), 240)
        .unwrap();
    assert_ne!(
        game.business_state_hash().unwrap(),
        with_plan,
        "执行腿变更必须独立于计划创建被检出"
    );
    let with_execution = game.business_state_hash().unwrap();
    let account = game.fixture_account_mut(&AccountId(1)).unwrap();
    account.set_strategy(Box::new(
        crate::MomentumStrategy::new(5, 0.04, 100).unwrap(),
    ));
    assert_ne!(
        game.business_state_hash().unwrap(),
        with_execution,
        "strategy 变更必须独立于计划/执行腿被检出"
    );
}

#[test]
fn diagnostic_metadata_changes_only_session_hash() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let business = game.business_state_hash().unwrap();
    let session = game.session_state_hash().unwrap();
    game.state
        .last_retail_order_events
        .push(RetailOrderDiagnosticEvent::Rejected {
            account: AccountId(1),
            code: StockId(0),
            reason: RejectionReason::InsufficientCash,
        });
    assert_eq!(game.business_state_hash().unwrap(), business);
    assert_ne!(game.session_state_hash().unwrap(), session);
}

#[test]
fn typed_private_plan_failure_is_poisoned_without_mutating_authority() {
    let code = StockId(0);
    let account = AccountId(1);
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.fixture_account_mut(&account)
        .unwrap()
        .fixture_set_strategy(None);
    game.fixture_account_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 2, Money::from_cents(1))
        .unwrap();
    game.state
        .markets
        .get_mut(&code)
        .unwrap()
        .fixture_set_last_price(Money::from_cents(i64::MAX));
    let business_before = game.business_state_hash().unwrap();
    let session_before = game.session_state_hash().unwrap();

    let fatal = match super::pipeline::plan_tick(super::pipeline::PhaseInput {
        session: &game,
    }) {
        Ok(_) => panic!("private plan must reject the overflowing allocation snapshot"),
        Err(fatal) => fatal,
    };
    let fatal = game.poison_failed_step(fatal);

    assert!(matches!(fatal, StepFatal::InvariantViolation { .. }));
    assert_eq!(game.business_state_hash().unwrap(), business_before);
    assert_ne!(game.session_state_hash().unwrap(), session_before);
    assert_eq!(game.poison_reason(), Some(&fatal));
    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.save().unwrap_err(), fatal);
}
