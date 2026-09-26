use crate::strategy::{ProductionStrategy, Strategy, StrategyState, StrategyStateError};
use std::sync::{Arc, OnceLock};

/// P2 works on a separate hydrated strategy and installs a replacement only after projection.
pub struct StoredStrategy(Arc<StrategyStorage>);

impl std::fmt::Debug for StoredStrategy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("StoredStrategy")
            .finish_non_exhaustive()
    }
}

impl Clone for StoredStrategy {
    fn clone(&self) -> Self {
        Self(Arc::clone(&self.0))
    }
}

struct StrategyStorage {
    strategy: Box<dyn ProductionStrategy>,
    validated_state: OnceLock<Result<StrategyState, StrategyStateError>>,
}

impl StoredStrategy {
    pub fn production(strategy: Box<dyn ProductionStrategy>) -> Self {
        Self(Arc::new(StrategyStorage {
            strategy,
            validated_state: OnceLock::new(),
        }))
    }

    pub(crate) fn production_validated(
        strategy: Box<dyn ProductionStrategy>,
        state: StrategyState,
    ) -> Self {
        let validated_state = OnceLock::new();
        validated_state
            .set(Ok(state))
            .expect("new strategy validation cache must be empty");
        Self(Arc::new(StrategyStorage {
            strategy,
            validated_state,
        }))
    }

    fn validated_state(&self) -> Result<&StrategyState, StrategyStateError> {
        self.0
            .validated_state
            .get_or_init(|| {
                // Export already validates parameters and strategy identity.
                // A second hydration would allocate and immediately discard a
                // duplicate strategy for every newly installed NPC state.
                StrategyState::from_strategy(self.0.strategy.as_ref())
            })
            .as_ref()
            .map_err(Clone::clone)
    }

    pub fn production_state(&self) -> Result<StrategyState, StrategyStateError> {
        self.validated_state().cloned()
    }

    pub(crate) fn validate_for_shadow(&self) -> Result<(), StrategyStateError> {
        self.validated_state()?;
        Ok(())
    }
}

impl std::ops::Deref for StoredStrategy {
    type Target = dyn Strategy;

    fn deref(&self) -> &Self::Target {
        self.0.strategy.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::{MomentumStrategy, ZiNoiseStrategy};

    #[test]
    fn quiet_shadow_shares_strategy_until_an_updated_instance_is_installed() {
        let stored =
            StoredStrategy::production(Box::new(ZiNoiseStrategy::new(0.3, 300, 0.4, 1).unwrap()));
        let before = stored.production_state().unwrap();
        stored.validate_for_shadow().unwrap();
        let mut shadow = stored.clone();
        assert!(std::ptr::eq::<dyn Strategy>(&*stored, &*shadow));

        shadow = StoredStrategy::production(Box::new(MomentumStrategy::new(5, 0.02, 100).unwrap()));
        assert!(!std::ptr::eq::<dyn Strategy>(&*stored, &*shadow));
        assert_eq!(stored.production_state().unwrap(), before);
        assert_ne!(shadow.production_state().unwrap(), before);
    }

    #[test]
    fn checked_production_strategy_carries_its_validation_into_the_next_shadow() {
        let strategy: Box<dyn ProductionStrategy> =
            Box::new(ZiNoiseStrategy::new(0.3, 300, 0.4, 1).unwrap());
        let state = StrategyState::from_strategy(strategy.as_ref()).unwrap();
        let stored = StoredStrategy::production_validated(strategy, state.clone());

        assert_eq!(stored.production_state().unwrap(), state);
        assert!(stored.0.validated_state.get().is_some());
        stored.validate_for_shadow().unwrap();
    }
}
