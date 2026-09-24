use crate::strategy::{ProductionStrategy, Strategy, StrategyState, StrategyStateError};
use std::sync::{Arc, OnceLock};

/// P2 works on a separate hydrated strategy and installs a replacement only after projection.
pub struct StoredStrategy(Arc<StrategyStorage>);

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

    fn validated_state(&self) -> Result<&StrategyState, StrategyStateError> {
        self.0
            .validated_state
            .get_or_init(|| {
                let state = StrategyState::from_strategy(self.0.strategy.as_ref())?;
                // The previous shadow path hydrated every stored strategy. Preserve that
                // validation once, then share the unchanged strategy across quiet ticks.
                state.clone().into_strategy()?;
                Ok(state)
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
}
