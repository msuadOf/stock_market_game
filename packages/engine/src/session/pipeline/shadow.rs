#[cfg(test)]
use super::Event;
use super::{GameSession, StepFatal};

pub struct TickShadow {
    session: Option<GameSession>,
    #[cfg(test)]
    non_authoritative_test_strategy: bool,
}

impl TickShadow {
    pub(super) fn capture(session: &GameSession) -> Result<Self, StepFatal> {
        match session.clone_for_tick_shadow() {
            Ok(session) => Ok(Self {
                session: Some(session),
                #[cfg(test)]
                non_authoritative_test_strategy: false,
            }),
            #[cfg(test)]
            Err(StepFatal::InvariantViolation { description, .. })
                if description
                    == crate::strategy::StrategyStateError::NonAuthoritative.to_string() =>
            {
                Ok(Self {
                    session: None,
                    non_authoritative_test_strategy: true,
                })
            }
            Err(error) => Err(error),
        }
    }

    #[cfg(test)]
    pub(super) fn run_compatibility_bridge(
        &mut self,
        skip_initial_npc_expiry: bool,
    ) -> Result<Vec<Event>, StepFatal> {
        self.session
            .as_mut()
            .map(|session| session.step_current_behavior(skip_initial_npc_expiry))
            .ok_or_else(|| StepFatal::InvariantViolation {
                description: "test-only non-authoritative strategy needs the compatibility bridge"
                    .to_owned(),
                location: "TickShadow::run_compatibility_bridge".to_owned(),
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
        self.session
            .as_mut()
            .ok_or_else(|| {
                E::from(StepFatal::InvariantViolation {
                    description:
                        "non-authoritative test strategy cannot execute a shadow operation"
                            .to_owned(),
                    location: "TickShadow::execute".to_owned(),
                })
            })
            .and_then(operation)
    }

    #[cfg(test)]
    pub(super) fn envelope_ledger(&self) -> Result<super::EnvelopeLedger, StepFatal> {
        self.session
            .as_ref()
            .map(|game| game.envelope_ledger.clone())
            .ok_or_else(|| StepFatal::InvariantViolation {
                description: "shadow has no envelope ledger".to_owned(),
                location: "TickShadow::envelope_ledger".to_owned(),
            })
    }

    #[cfg(test)]
    pub(super) fn commit_into(self, session: &mut GameSession) -> Result<(), StepFatal> {
        match self.session {
            Some(shadow) => {
                session.commit_tick_shadow(shadow);
                Ok(())
            }
            None => Err(StepFatal::InvariantViolation {
                description: "non-authoritative test strategy cannot commit a tick shadow"
                    .to_owned(),
                location: "TickShadow::commit_into".to_owned(),
            }),
        }
    }

    pub(super) fn into_session(self) -> Result<GameSession, StepFatal> {
        self.session.ok_or_else(|| StepFatal::InvariantViolation {
            description: "non-authoritative test strategy has no committable tick candidate"
                .to_owned(),
            location: "TickShadow::into_session".to_owned(),
        })
    }

    #[cfg(test)]
    pub(super) const fn is_non_authoritative_test_strategy(&self) -> bool {
        self.non_authoritative_test_strategy
    }

    #[cfg(not(test))]
    pub(super) const fn is_non_authoritative_test_strategy(&self) -> bool {
        false
    }

    #[cfg(test)]
    pub(super) fn strategy_state(
        &self,
        account: crate::AccountId,
    ) -> Result<crate::strategy::StrategyState, StepFatal> {
        self.session
            .as_ref()
            .ok_or_else(|| StepFatal::InvariantViolation {
                description: "shadow has no production strategy state".to_owned(),
                location: "TickShadow::strategy_state".to_owned(),
            })?
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
