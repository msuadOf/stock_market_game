use crate::strategy::{ProductionStrategy, Strategy, StrategyState, StrategyStateError};

/// Keeps sealed export authority separate from non-authoritative decision fixtures.
pub struct StoredStrategy(Storage);

enum Storage {
    Production(Box<dyn ProductionStrategy>),
    NonAuthoritative(Box<dyn Strategy>),
}

impl StoredStrategy {
    pub fn production(strategy: Box<dyn ProductionStrategy>) -> Self {
        Self(Storage::Production(strategy))
    }

    /// Decision-only adapter; export always fails, even if the profile looks valid.
    pub fn non_authoritative(strategy: Box<dyn Strategy>) -> Self {
        Self(Storage::NonAuthoritative(strategy))
    }

    pub fn production_state(&self) -> Result<StrategyState, StrategyStateError> {
        match &self.0 {
            Storage::Production(strategy) => StrategyState::from_strategy(strategy.as_ref()),
            Storage::NonAuthoritative(_) => Err(StrategyStateError::NonAuthoritative),
        }
    }

    pub(crate) fn clone_for_shadow(&self) -> Result<Self, StrategyStateError> {
        Ok(Self::production(self.production_state()?.into_strategy()?))
    }

    pub(crate) const fn is_production(&self) -> bool {
        matches!(self.0, Storage::Production(_))
    }
}

impl std::ops::Deref for StoredStrategy {
    type Target = dyn Strategy;

    fn deref(&self) -> &Self::Target {
        match &self.0 {
            Storage::Production(strategy) => strategy.as_ref(),
            Storage::NonAuthoritative(strategy) => strategy.as_ref(),
        }
    }
}

impl std::ops::DerefMut for StoredStrategy {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match &mut self.0 {
            Storage::Production(strategy) => strategy.as_mut(),
            Storage::NonAuthoritative(strategy) => strategy.as_mut(),
        }
    }
}
