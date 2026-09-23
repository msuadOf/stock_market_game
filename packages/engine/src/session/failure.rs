use super::{Event, GameSession, SaveSlot};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, thiserror::Error)]
pub enum StepFatal {
    #[error("invariant violation at {location}: {description}")]
    InvariantViolation {
        description: String,
        location: String,
    },
    #[error("rollback hash mismatch: expected {expected:?}, observed {observed:?}")]
    Internal {
        expected: super::StateHash,
        observed: super::StateHash,
    },
}

impl GameSession {
    pub fn publication_ids(&self) -> Vec<crate::information::PublicationId> {
        self.library.publication_ids()
    }
    pub const fn poison_reason(&self) -> Option<&StepFatal> {
        self.poison.as_ref()
    }

    pub(super) fn require_healthy(&self) -> Result<(), StepFatal> {
        match &self.poison {
            Some(fatal) => Err(fatal.clone()),
            None => Ok(()),
        }
    }

    /// Typed phase failures poison only after both rollback projections prove no state leaked.
    pub fn step(&mut self) -> Result<Vec<Event>, StepFatal> {
        self.require_healthy()?;
        let rollback_before = match self.rollback_hashes() {
            Ok(hash) => Some(hash),
            #[cfg(test)]
            Err(StepFatal::InvariantViolation { location, .. })
                if location == "state_hash.strategy" =>
            {
                // Legacy unit-test strategies are intentionally non-authoritative and cannot
                // participate in the production rollback hash contract.
                None
            }
            Err(fatal) => {
                self.poison = Some(fatal.clone());
                return Err(fatal);
            }
        };
        #[cfg(test)]
        if self.injected_failure.is_some() {
            if let Err(fatal) = self.run_pre_mutation_hook() {
                return Err(self.poison_failed_step(rollback_before, fatal));
            }
        }
        let result = super::pipeline::plan_tick(super::pipeline::PhaseInput { session: self })
            .and_then(|shadow| super::pipeline::commit_tick(self, shadow));
        match result {
            Ok(committed) => Ok(committed.events),
            Err(fatal) => Err(self.poison_failed_step(rollback_before, fatal)),
        }
    }

    pub(super) fn poison_failed_step(
        &mut self,
        rollback_before: Option<super::hash::RollbackHashes>,
        fatal: StepFatal,
    ) -> StepFatal {
        let verified = match rollback_before {
            Some(expected) => match self.rollback_hashes() {
                Ok(observed) if observed == expected => fatal,
                Ok(observed) if observed.business != expected.business => StepFatal::Internal {
                    expected: expected.business,
                    observed: observed.business,
                },
                Ok(observed) => StepFatal::Internal {
                    expected: expected.session,
                    observed: observed.session,
                },
                Err(hash_error) => hash_error,
            },
            None => fatal,
        };
        self.poison = Some(verified.clone());
        verified
    }

    pub fn save(&self) -> Result<SaveSlot, StepFatal> {
        self.require_healthy()?;
        for account in self.accounts.values() {
            if account
                .strategy
                .as_ref()
                .is_some_and(|strategy| !strategy.is_production())
            {
                return Err(StepFatal::InvariantViolation {
                    description: format!(
                        "account {} has a non-authoritative strategy",
                        account.id.0
                    ),
                    location: "GameSession::save".to_owned(),
                });
            }
        }
        Ok(self.save_projection())
    }

    #[cfg(test)]
    pub(super) fn inject_step_failure(&mut self, fatal: StepFatal) {
        self.injected_failure = Some(fatal);
    }

    #[cfg(test)]
    pub(super) fn inject_post_shadow_failure(&mut self, fatal: StepFatal) {
        self.post_shadow_failure = Some(fatal);
    }

    #[cfg(test)]
    fn run_pre_mutation_hook(&mut self) -> Result<(), StepFatal> {
        match self.injected_failure.take() {
            Some(fatal) => Err(fatal),
            None => Ok(()),
        }
    }

    #[cfg(test)]
    pub(super) fn run_post_shadow_hook(&mut self) -> Result<(), StepFatal> {
        match self.post_shadow_failure.take() {
            Some(fatal) => Err(fatal),
            None => Ok(()),
        }
    }
}
