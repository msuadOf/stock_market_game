use engine::strategy::{InstitutionStyle, ProductionStrategy, StrategyState};
use engine::{
    BeliefInstitutionStrategy, MomentumStrategy, Strategy, StrategyFamily, StrategyProfile,
    ZiNoiseStrategy,
};

#[test]
fn strategy_state_round_trip_preserves_concrete_parameters(
) -> Result<(), Box<dyn std::error::Error>> {
    let mut strategies: Vec<Box<dyn ProductionStrategy>> = vec![
        Box::new(ZiNoiseStrategy::new(0.37, 300, 0.21, 2)?),
        Box::new(MomentumStrategy::new(7, 0.023, 400)?),
        Box::new(BeliefInstitutionStrategy::new(0.07, 500)?),
    ];
    let params = engine::StrategyParams {
        retail: engine::RetailParams {
            arrival_rate: 0.3,
            order_size_mean: 100,
            chase_prob: 0.2,
            tick_cents: 1,
        },
        inst: engine::InstParams {
            margin: 0.03,
            order_size: 200,
        },
        hot: engine::HotParams {
            lookback: 5,
            trend_threshold: 0.02,
            order_size: 300,
        },
    };
    let mut rng = engine::SplitMix64::new(42);
    let institution = engine::StrategyFactory::build_for_market_day_with_ordinal(
        engine::AccountKind::Inst,
        &params,
        240,
        4,
        &mut rng,
    )?
    .ok_or("institution strategy missing")?;
    assert!(matches!(
        StrategyState::from_strategy(institution.as_ref())?,
        StrategyState::InstitutionMomentum(_)
    ));
    strategies.push(institution);
    for kind in [
        engine::AccountKind::Retail,
        engine::AccountKind::Inst,
        engine::AccountKind::Hot,
    ] {
        for ordinal in 0..12 {
            let sampled = engine::StrategyFactory::build_for_market_day_with_ordinal(
                kind, &params, 240, ordinal, &mut rng,
            )?
            .ok_or("NPC strategy missing")?;
            strategies.push(sampled);
        }
    }
    for strategy in strategies {
        let state = StrategyState::from_strategy(strategy.as_ref())?;
        let mut account = engine::Account::new(
            engine::AccountId(1),
            engine::AccountKind::Inst,
            engine::Money::ZERO,
        );
        account.set_strategy(strategy);
        let stored = account.strategy.as_ref().ok_or("stored strategy missing")?;
        assert_eq!(stored.production_state()?, state);
        let strategy = state.clone().into_strategy()?;
        let encoded = serde_json::to_vec(&state)?;
        let decoded: StrategyState = serde_json::from_slice(&encoded)?;
        assert_eq!(state, decoded);
        let restored = decoded.into_strategy()?;
        assert_eq!(restored.profile(), strategy.profile());
        assert_eq!(restored.strategy_family(), strategy.strategy_family());
        assert_eq!(StrategyState::from_strategy(restored.as_ref())?, state);
        assert_eq!(serde_json::to_vec(&state)?, encoded);
        println!(
            "roundtrip profile={:?} family={:?} state={}",
            restored.profile(),
            restored.strategy_family(),
            String::from_utf8(encoded)?
        );
    }
    Ok(())
}

struct UnknownStrategy;

impl Strategy for UnknownStrategy {
    fn profile(&self) -> StrategyProfile {
        StrategyProfile::Institution(InstitutionStyle::Balanced)
    }

    fn strategy_family(&self) -> StrategyFamily {
        StrategyFamily::FundamentalValue
    }

    fn decide(
        &mut self,
        _market: &engine::MarketView,
        _own: &engine::SelfView,
        _rng: &mut dyn engine::Rng,
    ) -> Vec<engine::Intent> {
        Vec::new()
    }
}

#[test]
fn decision_trait_remains_extensible_and_unknown_wire_variant_is_rejected() {
    let custom: Box<dyn Strategy> = Box::new(UnknownStrategy);
    assert_eq!(custom.strategy_family(), StrategyFamily::FundamentalValue);
    assert!(serde_json::from_str::<StrategyState>(r#"{"Unknown":{}}"#).is_err());
}

#[test]
fn strategy_state_rejects_non_finite_parameter_bits() -> Result<(), Box<dyn std::error::Error>> {
    let state = StrategyState::from_strategy(&MomentumStrategy::new(5, 0.02, 300)?)?;
    let encoded = serde_json::to_string(&state)?;
    let malformed = encoded.replace("3f947ae147ae147b", "7ff0000000000000");
    assert_ne!(encoded, malformed);
    let error =
        serde_json::from_str::<StrategyState>(&malformed).expect_err("nonfinite state must fail");
    println!("malformed nonfinite state: ERROR {error}");
    Ok(())
}

#[test]
fn strategy_state_rejects_invalid_known_parameters_before_reconstruction(
) -> Result<(), Box<dyn std::error::Error>> {
    for (strategy, field) in [
        (
            Box::new(MomentumStrategy::new(5, 0.02, 300)?) as Box<dyn ProductionStrategy>,
            "order_size",
        ),
        (
            Box::new(ZiNoiseStrategy::new(0.3, 100, 0.2, 1)?) as Box<dyn ProductionStrategy>,
            "order_size_mean",
        ),
        (
            Box::new(BeliefInstitutionStrategy::new(0.03, 200)?) as Box<dyn ProductionStrategy>,
            "order_size",
        ),
    ] {
        let mut encoded = serde_json::to_value(StrategyState::from_strategy(strategy.as_ref())?)?;
        let variant = encoded
            .as_object_mut()
            .ok_or("state object missing")?
            .values_mut()
            .next()
            .ok_or("variant missing")?;
        variant[field] = serde_json::json!(0);
        let state: StrategyState = serde_json::from_value(encoded)?;
        assert!(matches!(
            state.into_strategy(),
            Err(engine::strategy::StrategyStateError::InvalidParameters(_))
        ));
    }
    Ok(())
}
