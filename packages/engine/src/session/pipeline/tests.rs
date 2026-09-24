use super::*;

#[test]
fn discarded_shadow_preserves_business_hash() {
    let game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let before = game.business_state_hash().unwrap();
    let plan = plan_tick(PhaseInput { session: &game }).unwrap();
    assert!(plan.expiry_applied);
    assert!(plan.decision_resources.is_some());
    drop(plan);
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
    let before = game.business_state_hash().unwrap();
    let tick_before = game.tick();

    let events = game.step().unwrap();

    assert!(!events.is_empty());
    assert_eq!(game.tick(), tick_before + 1);
    assert_ne!(game.business_state_hash().unwrap(), before);
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
