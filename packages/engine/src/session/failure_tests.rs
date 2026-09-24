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
    game.accounts.get_mut(&AccountId(1)).unwrap().strategy = None;
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
    game.accounts.get_mut(&AccountId(1)).unwrap().strategy = None;
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
    game.next_order_id += 1;
    assert_ne!(game.business_state_hash().unwrap(), before);
    game.next_order_id -= 1;
    game.next_receipt_base += 1;
    assert_ne!(game.business_state_hash().unwrap(), before);
    game.next_receipt_base -= 1;
    game.pending_plan_events.push(PendingPlanEvent::DayEnded {
        plan_id: crate::plans::PlanId(999),
        trading_day: 0,
    });
    assert_ne!(game.business_state_hash().unwrap(), before);
    game.pending_plan_events.clear();
    let account = game.accounts.get_mut(&AccountId(1)).unwrap();
    account.set_strategy(Box::new(
        crate::MomentumStrategy::new(5, 0.04, 100).unwrap(),
    ));
    assert_ne!(game.business_state_hash().unwrap(), before);
}

#[test]
fn diagnostic_metadata_changes_only_session_hash() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let business = game.business_state_hash().unwrap();
    let session = game.session_state_hash().unwrap();
    game.last_retail_order_events
        .push(RetailOrderDiagnosticEvent::Rejected {
            account: AccountId(1),
            code: StockCode("600888".to_owned()),
            reason: RejectionReason::InsufficientCash,
        });
    assert_eq!(game.business_state_hash().unwrap(), business);
    assert_ne!(game.session_state_hash().unwrap(), session);
}

#[test]
fn typed_private_plan_failure_is_poisoned_without_mutating_authority() {
    let code = StockCode("600888".to_owned());
    let account = AccountId(1);
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts.get_mut(&account).unwrap().strategy = None;
    game.accounts
        .get_mut(&account)
        .unwrap()
        .grant_position(code.clone(), 2, Money::from_cents(1))
        .unwrap();
    game.markets
        .get_mut(&code)
        .unwrap()
        .set_last_price(Money::from_cents(i64::MAX));
    let business_before = game.business_state_hash().unwrap();
    let session_before = game.session_state_hash().unwrap();

    let fatal = match super::pipeline::plan_tick(super::pipeline::PhaseInput { session: &game }) {
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
