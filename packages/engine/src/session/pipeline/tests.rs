use super::*;

#[test]
fn step_phases_public_step_calls_commit_once() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    COMMIT_TRACES.with_borrow_mut(Vec::clear);
    game.step().unwrap();
    COMMIT_TRACES.with_borrow(|traces| assert_eq!(traces, &[TickPhase::ALL.to_vec()]));
}

#[test]
fn discarded_shadow_preserves_business_hash() {
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let before = game.business_state_hash().unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    assert_eq!(plan.trace(), TickPhase::ALL[..9]);
    drop(plan);
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn planned_shadow_preserves_authority_until_commit() {
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(1), 42).unwrap();
    let before = game.business_state_hash().unwrap();

    let shadow = plan_tick(PhaseInput { session: &game }).unwrap();

    assert_eq!(shadow.trace(), TickPhase::ALL[..9]);
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn post_shadow_fatal_preserves_authority_and_publishes_no_events() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(1), 42).unwrap();
    let before = game.business_state_hash().unwrap();
    let fatal = StepFatal::InvariantViolation {
        description: "post-shadow test failure".to_owned(),
        location: "pipeline::tests".to_owned(),
    };
    game.inject_post_shadow_failure(fatal.clone());

    assert_eq!(game.step().unwrap_err(), fatal);
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn committed_shadow_advances_authority_once() {
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(1), 42).unwrap();
    COMMIT_TRACES.with_borrow_mut(Vec::clear);
    let before = game.business_state_hash().unwrap();

    let events = game.step().unwrap();

    assert!(!events.is_empty());
    assert_ne!(game.business_state_hash().unwrap(), before);
    COMMIT_TRACES.with_borrow(|traces| assert_eq!(traces, &[TickPhase::ALL.to_vec()]));
}

#[test]
fn shadow_capture_rehydrates_production_strategy_without_identity_drift() {
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(1), 42).unwrap();
    let before = game.accounts[&crate::AccountId(1)]
        .strategy
        .as_ref()
        .unwrap()
        .production_state()
        .unwrap();

    let shadow = plan_tick(PhaseInput { session: &game }).unwrap();

    assert_eq!(shadow.strategy_state(crate::AccountId(1)).unwrap(), before);
}
