use super::{GameSession, StepFatal};

pub struct TickShadow {
    session: Option<GameSession>,
}

impl TickShadow {
    pub(super) fn capture(session: &GameSession) -> Result<Self, StepFatal> {
        Ok(Self {
            session: Some(session.clone_for_tick_shadow()?),
        })
    }

    pub(super) fn execute<R>(
        &mut self,
        operation: impl FnOnce(&mut GameSession) -> Result<R, StepFatal>,
    ) -> Result<R, StepFatal> {
        self.execute_typed(operation)
    }

    pub(super) fn execute_typed<R, E>(
        &mut self,
        operation: impl FnOnce(&mut GameSession) -> Result<R, E>,
    ) -> Result<R, E>
    where
        E: From<StepFatal>,
    {
        operation(self.session.as_mut().ok_or_else(consumed_shadow)?)
    }

    /// Transfer the discardable candidate into a phase transaction. A failure drops it;
    /// restoring a partially changed candidate would make a failed plan committable.
    pub(super) fn take_session(&mut self) -> Result<GameSession, StepFatal> {
        self.session.take().ok_or_else(consumed_shadow)
    }

    pub(super) fn restore_success(&mut self, session: GameSession) -> Result<(), StepFatal> {
        if self.session.is_some() {
            return Err(StepFatal::InvariantViolation {
                description: "phase returned a candidate to an occupied tick shadow".to_owned(),
                location: "TickShadow::restore_success".to_owned(),
            });
        }
        self.session = Some(session);
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn envelope_ledger(&self) -> Result<super::EnvelopeLedger, StepFatal> {
        Ok(self
            .session
            .as_ref()
            .ok_or_else(consumed_shadow)?
            .envelope_ledger
            .clone())
    }

    pub(super) fn into_session(self) -> Result<GameSession, StepFatal> {
        self.session.ok_or_else(consumed_shadow)
    }

    #[cfg(test)]
    pub(super) fn strategy_state(
        &self,
        account: crate::AccountId,
    ) -> Result<crate::strategy::StrategyState, StepFatal> {
        self.session
            .as_ref()
            .ok_or_else(consumed_shadow)?
            .accounts
            .get(&account)
            .and_then(|account| account.strategy.as_ref())
            .ok_or_else(|| StepFatal::InvariantViolation {
                description: format!("shadow strategy missing for account {}", account.0),
                location: "TickShadow::strategy_state".to_owned(),
            })?
            .production_state()
            .map_err(|error| StepFatal::InvariantViolation {
                description: error.to_string(),
                location: "TickShadow::strategy_state".to_owned(),
            })
    }
}

fn consumed_shadow() -> StepFatal {
    StepFatal::InvariantViolation {
        description: "tick shadow was consumed by a failed phase transaction".to_owned(),
        location: "TickShadow".to_owned(),
    }
}
