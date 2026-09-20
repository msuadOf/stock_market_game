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
fn non_authoritative_strategy_executes_but_cannot_export_or_hash() {
    struct Idle;
    impl crate::Strategy for Idle {
        fn profile(&self) -> StrategyProfile {
            StrategyProfile::Hot(crate::strategy::HotStyle::Momentum)
        }
        fn strategy_family(&self) -> crate::StrategyFamily {
            crate::StrategyFamily::Momentum
        }
        fn decide(&mut self, _: &MarketView, _: &SelfView, _: &mut dyn Rng) -> Vec<Intent> {
            Vec::new()
        }
    }
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.accounts.get_mut(&AccountId(1)).unwrap().strategy = Some(
        crate::account::StoredStrategy::non_authoritative(Box::new(Idle)),
    );
    assert!(game.step().is_ok());
    assert_eq!(
        game.save().unwrap_err(),
        StepFatal::InvariantViolation {
            description: "account 1 has a non-authoritative strategy".to_owned(),
            location: "GameSession::save".to_owned(),
        }
    );
    assert!(
        matches!(game.business_state_hash(), Err(StepFatal::InvariantViolation { location, .. }) if location == "state_hash.strategy")
    );
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
fn hashes_repeat_for_identical_seed_and_successful_steps() {
    let setup = super::npc_working_quote_tests::quote_setup(0);
    let mut first = GameSession::new(setup.clone(), 42).unwrap();
    let mut second = GameSession::new(setup, 42).unwrap();
    for _ in 0..5 {
        first.step().unwrap();
        second.step().unwrap();
        assert_eq!(
            first.business_state_hash().unwrap(),
            second.business_state_hash().unwrap()
        );
        assert_eq!(
            first.session_state_hash().unwrap(),
            second.session_state_hash().unwrap()
        );
    }
}

#[test]
fn typed_private_plan_failure_is_poisoned_after_dual_hash_confirms_rollback() {
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

    let rollback_before = Some(game.rollback_hashes().unwrap());
    let fatal = match super::pipeline::plan_tick(super::pipeline::PhaseInput { session: &game }) {
        Ok(_) => panic!("private plan must reject the overflowing allocation snapshot"),
        Err(fatal) => fatal,
    };
    let fatal = game.poison_failed_step(rollback_before, fatal);

    assert!(matches!(fatal, StepFatal::InvariantViolation { .. }));
    assert_eq!(game.business_state_hash().unwrap(), business_before);
    assert_ne!(game.session_state_hash().unwrap(), session_before);
    assert_eq!(game.poison_reason(), Some(&fatal));
    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.save().unwrap_err(), fatal);
}

#[test]
fn changed_business_hash_escalates_and_persists_an_internal_fatal() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let expected = game.rollback_hashes().unwrap();
    game.next_order_id = game.next_order_id.checked_add(1).unwrap();
    let observed = game.business_state_hash().unwrap();
    let original = StepFatal::InvariantViolation {
        description: "typed failure after forbidden authority mutation".to_owned(),
        location: "failure_tests::hash_mismatch".to_owned(),
    };

    let fatal = game.poison_failed_step(Some(expected), original);

    assert_eq!(
        fatal,
        StepFatal::Internal {
            expected: expected.business,
            observed,
        }
    );
    assert_eq!(game.poison_reason(), Some(&fatal));
    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.save().unwrap_err(), fatal);
}

#[test]
fn changed_diagnostic_session_hash_escalates_instead_of_leaking_through_poison() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let expected = game.rollback_hashes().unwrap();
    game.last_retail_order_events
        .push(RetailOrderDiagnosticEvent::Rejected {
            account: AccountId(1),
            code: StockCode("600888".to_owned()),
            reason: RejectionReason::InsufficientCash,
        });
    let observed = game.session_state_hash().unwrap();
    let original = StepFatal::InvariantViolation {
        description: "typed failure after forbidden diagnostic mutation".to_owned(),
        location: "failure_tests::session_hash_mismatch".to_owned(),
    };

    let fatal = game.poison_failed_step(Some(expected), original);

    assert_eq!(
        fatal,
        StepFatal::Internal {
            expected: expected.session,
            observed,
        }
    );
    assert_eq!(game.poison_reason(), Some(&fatal));
}

#[test]
fn rollback_witness_detects_business_state_leaking_before_commit() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let witness = game.clone_for_tick_shadow().unwrap();
    let expected = witness.rollback_hashes().unwrap();
    game.next_order_id = game.next_order_id.checked_add(1).unwrap();
    let observed = game.business_state_hash().unwrap();
    let original = StepFatal::InvariantViolation {
        description: "typed failure after leaked business mutation".to_owned(),
        location: "failure_tests::rollback_witness_business".to_owned(),
    };

    let fatal = game.poison_failed_step_from_witness(Some(witness), original);

    assert_eq!(
        fatal,
        StepFatal::Internal {
            expected: expected.business,
            observed,
        }
    );
    assert_eq!(game.poison_reason(), Some(&fatal));
}

#[test]
fn rollback_witness_detects_session_state_leaking_before_commit() {
    let mut game = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let witness = game.clone_for_tick_shadow().unwrap();
    let expected = witness.rollback_hashes().unwrap();
    game.last_retail_order_events
        .push(RetailOrderDiagnosticEvent::Rejected {
            account: AccountId(1),
            code: StockCode("600888".to_owned()),
            reason: RejectionReason::InsufficientCash,
        });
    let observed = game.session_state_hash().unwrap();
    let original = StepFatal::InvariantViolation {
        description: "typed failure after leaked session mutation".to_owned(),
        location: "failure_tests::rollback_witness_session".to_owned(),
    };

    let fatal = game.poison_failed_step_from_witness(Some(witness), original);

    assert_eq!(
        fatal,
        StepFatal::Internal {
            expected: expected.session,
            observed,
        }
    );
    assert_eq!(game.poison_reason(), Some(&fatal));
}
