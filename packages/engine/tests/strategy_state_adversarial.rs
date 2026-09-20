use engine::strategy::{HotStyle, InstitutionStyle, StrategyState, StrategyStateError};
use engine::MomentumStrategy;

#[test]
fn impossible_registered_identities_are_rejected() -> Result<(), Box<dyn std::error::Error>> {
    let belief = engine::BeliefInstitutionStrategy::new(0.03, 100)?
        .with_institution_style(InstitutionStyle::ActiveTrader);
    assert_eq!(
        StrategyState::from_strategy(&belief),
        Err(StrategyStateError::IdentityMismatch)
    );
    Ok(())
}

#[test]
fn institution_momentum_rejects_non_active_outer_style() -> Result<(), Box<dyn std::error::Error>> {
    let inner = serde_json::to_value(StrategyState::from_strategy(&MomentumStrategy::new(
        5, 0.02, 100,
    )?)?)?;
    let state: StrategyState = serde_json::from_value(serde_json::json!({"InstitutionMomentum": {
        "style": "Balanced", "inner": inner["Momentum"]
    }}))?;
    assert!(matches!(
        state.into_strategy(),
        Err(StrategyStateError::IdentityMismatch)
    ));
    Ok(())
}

#[test]
fn institution_momentum_rejects_reversal_inner_style() -> Result<(), Box<dyn std::error::Error>> {
    let mut inner = serde_json::to_value(StrategyState::from_strategy(&MomentumStrategy::new(
        5, 0.02, 100,
    )?)?)?;
    inner["Momentum"]["style"] = serde_json::json!(HotStyle::Reversal);
    let state: StrategyState = serde_json::from_value(serde_json::json!({"InstitutionMomentum": {
        "style": "ActiveTrader", "inner": inner["Momentum"]
    }}))?;
    assert!(matches!(
        state.into_strategy(),
        Err(StrategyStateError::IdentityMismatch)
    ));
    Ok(())
}
