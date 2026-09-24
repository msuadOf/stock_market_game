use super::{
    momentum::InstitutionMomentumStrategy, BeliefInstitutionStrategy, HotStyle, InstitutionStyle,
    MomentumStrategy, Strategy, StrategyFamily, StrategyProfile, ZiNoiseStrategy,
};

mod sealed {
    pub trait Registered {}
    impl Registered for super::ZiNoiseStrategy {}
    impl Registered for super::MomentumStrategy {}
    impl Registered for super::InstitutionMomentumStrategy {}
    impl Registered for super::BeliefInstitutionStrategy {}
}

/// Export authority for the four engine-owned production implementations.
/// External decision strategies cannot implement this sealed capability.
///
/// ```compile_fail
/// use engine::{Strategy, MomentumStrategy, StrategyProfile, StrategyFamily};
/// use engine::strategy::{ProductionStrategy, StrategyState, StrategyStateError, HotStyle};
/// struct Forged;
/// impl Strategy for Forged {
///     fn profile(&self) -> StrategyProfile { StrategyProfile::Hot(HotStyle::Momentum) }
///     fn strategy_family(&self) -> StrategyFamily { StrategyFamily::Momentum }
///     fn decide(&mut self, _: &engine::MarketView, _: &engine::SelfView, _: &mut dyn engine::Rng) -> Vec<engine::Intent> { vec![] }
/// }
/// impl ProductionStrategy for Forged {
///     fn state(&self) -> Result<StrategyState, StrategyStateError> {
///         MomentumStrategy::new(5, 0.02, 100)
///             .map(StrategyState::Momentum)
///             .map_err(|error| StrategyStateError::InvalidParameters(error.to_string()))
///     }
/// }
/// ```
pub trait ProductionStrategy: Strategy + sealed::Registered {
    fn state(&self) -> Result<StrategyState, StrategyStateError>;
}

pub(super) mod exact_float {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &f64, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{:016x}", value.to_bits()))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        let encoded = String::deserialize(deserializer)?;
        if encoded.len() != 16
            || !encoded
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(serde::de::Error::custom(
                "strategy parameter must use exactly 16 lowercase hexadecimal digits",
            ));
        }
        let bits = u64::from_str_radix(&encoded, 16).map_err(serde::de::Error::custom)?;
        let value = f64::from_bits(bits);
        if !value.is_finite() {
            return Err(serde::de::Error::custom(
                "strategy parameter must be finite",
            ));
        }
        Ok(value)
    }
}

pub(super) const MAX_JAVASCRIPT_SAFE_INTEGER: u64 = 9_007_199_254_740_991;

pub(super) mod js_safe_usize {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &usize, serializer: S) -> Result<S::Ok, S::Error> {
        if u64::try_from(*value).map_or(true, |value| value > super::MAX_JAVASCRIPT_SAFE_INTEGER) {
            return Err(serde::ser::Error::custom(
                "strategy integer exceeds the JavaScript safe integer range",
            ));
        }
        serializer.serialize_u64(*value as u64)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<usize, D::Error> {
        let value = u64::deserialize(deserializer)?;
        if value > super::MAX_JAVASCRIPT_SAFE_INTEGER {
            return Err(serde::de::Error::custom(
                "strategy integer exceeds the JavaScript safe integer range",
            ));
        }
        usize::try_from(value).map_err(serde::de::Error::custom)
    }
}

pub(super) mod js_safe_i64 {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(value: &i64, serializer: S) -> Result<S::Ok, S::Error> {
        if value.unsigned_abs() > super::MAX_JAVASCRIPT_SAFE_INTEGER {
            return Err(serde::ser::Error::custom(
                "strategy integer exceeds the JavaScript safe integer range",
            ));
        }
        serializer.serialize_i64(*value)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
        let value = i64::deserialize(deserializer)?;
        if value.unsigned_abs() > super::MAX_JAVASCRIPT_SAFE_INTEGER {
            return Err(serde::de::Error::custom(
                "strategy integer exceeds the JavaScript safe integer range",
            ));
        }
        Ok(value)
    }
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub enum StrategyState {
    ZiNoise(ZiNoiseStrategy),
    Momentum(MomentumStrategy),
    InstitutionMomentum(InstitutionMomentumStrategy),
    BeliefInstitution(BeliefInstitutionStrategy),
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum StrategyStateError {
    #[error("invalid strategy state: {0}")]
    InvalidParameters(String),
    #[error("strategy state identity does not match the source strategy")]
    IdentityMismatch,
}

impl StrategyState {
    pub fn from_strategy(strategy: &dyn ProductionStrategy) -> Result<Self, StrategyStateError> {
        let state = strategy.state()?;
        state.validate()?;
        if state.profile() != strategy.profile() || state.family() != strategy.strategy_family() {
            return Err(StrategyStateError::IdentityMismatch);
        }
        Ok(state)
    }

    fn validate(&self) -> Result<(), StrategyStateError> {
        match self {
            Self::ZiNoise(strategy) => strategy.validate_state()?,
            Self::Momentum(strategy) => strategy.validate_state()?,
            Self::InstitutionMomentum(strategy) => {
                match (strategy.style, strategy.inner.style) {
                    (InstitutionStyle::ActiveTrader, HotStyle::Momentum) => {}
                    (
                        InstitutionStyle::DeepValue
                        | InstitutionStyle::Growth
                        | InstitutionStyle::Balanced
                        | InstitutionStyle::Defensive,
                        _,
                    )
                    | (InstitutionStyle::ActiveTrader, HotStyle::Reversal) => {
                        return Err(StrategyStateError::IdentityMismatch)
                    }
                }
                strategy.inner.validate_state()?;
            }
            Self::BeliefInstitution(strategy) => {
                match strategy.style {
                    InstitutionStyle::ActiveTrader => {
                        return Err(StrategyStateError::IdentityMismatch)
                    }
                    InstitutionStyle::DeepValue
                    | InstitutionStyle::Growth
                    | InstitutionStyle::Balanced
                    | InstitutionStyle::Defensive => {}
                }
                strategy.validate_state()?;
            }
        }
        Ok(())
    }

    pub fn into_strategy(self) -> Result<Box<dyn ProductionStrategy>, StrategyStateError> {
        self.validate()?;
        Ok(match self {
            Self::ZiNoise(strategy) => Box::new(strategy),
            Self::Momentum(strategy) => Box::new(strategy),
            Self::InstitutionMomentum(strategy) => Box::new(strategy),
            Self::BeliefInstitution(strategy) => Box::new(strategy),
        })
    }

    pub fn profile(&self) -> StrategyProfile {
        match self {
            Self::ZiNoise(strategy) => strategy.profile(),
            Self::Momentum(strategy) => strategy.profile(),
            Self::InstitutionMomentum(strategy) => strategy.profile(),
            Self::BeliefInstitution(strategy) => strategy.profile(),
        }
    }

    pub fn family(&self) -> StrategyFamily {
        match self {
            Self::ZiNoise(_) => StrategyFamily::RetailBehavior,
            Self::Momentum(_) | Self::InstitutionMomentum(_) => StrategyFamily::Momentum,
            Self::BeliefInstitution(_) => StrategyFamily::FundamentalValue,
        }
    }

    pub(crate) fn base_observation_probability(&self) -> f64 {
        match self {
            Self::ZiNoise(strategy) => strategy.base_observation_probability,
            Self::Momentum(strategy) => strategy.base_observation_probability,
            Self::InstitutionMomentum(strategy) => strategy.inner.base_observation_probability,
            Self::BeliefInstitution(strategy) => strategy.base_observation_probability,
        }
    }

    pub(crate) fn set_base_observation_probability(&mut self, probability: f64) {
        match self {
            Self::ZiNoise(strategy) => strategy.base_observation_probability = probability,
            Self::Momentum(strategy) => strategy.base_observation_probability = probability,
            Self::InstitutionMomentum(strategy) => {
                strategy.inner.base_observation_probability = probability;
            }
            Self::BeliefInstitution(strategy) => {
                strategy.base_observation_probability = probability;
            }
        }
    }
}
