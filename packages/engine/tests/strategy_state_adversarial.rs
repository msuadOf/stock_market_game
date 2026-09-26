use engine::strategy::{InstitutionStyle, StrategyFamily, StrategyState};

#[test]
fn active_trader_is_a_plan_strategy_and_old_momentum_state_is_rejected(
) -> Result<(), Box<dyn std::error::Error>> {
    let strategy = engine::BeliefInstitutionStrategy::new(0.03, 100)?
        .with_institution_style(InstitutionStyle::ActiveTrader);
    let state = StrategyState::from_strategy(&strategy)?;
    assert!(matches!(&state, StrategyState::BeliefInstitution(_)));
    assert_eq!(state.family(), StrategyFamily::Momentum);
    assert!(serde_json::from_str::<StrategyState>(
        r#"{"InstitutionMomentum":{"style":"ActiveTrader","inner":{}}}"#
    )
    .is_err());
    Ok(())
}
