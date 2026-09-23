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

    /// Typed phase failures poison the session only after the business hash proves that the
    /// discardable tick left authority unchanged. Panics remain process-level failures.
    pub fn step(&mut self) -> Result<Vec<Event>, StepFatal> {
        self.require_healthy()?;
        let business_before = match self.business_state_hash() {
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
                return Err(self.poison_failed_step(business_before, fatal));
            }
        }
        let result = super::pipeline::plan_tick(super::pipeline::PhaseInput { session: self })
            .and_then(|shadow| super::pipeline::commit_tick(self, shadow));
        match result {
            Ok(committed) => Ok(committed.events),
            Err(fatal) => Err(self.poison_failed_step(business_before, fatal)),
        }
    }

    pub(super) fn poison_failed_step(
        &mut self,
        business_before: Option<super::StateHash>,
        fatal: StepFatal,
    ) -> StepFatal {
        let verified = match business_before {
            Some(expected) => match self.business_state_hash() {
                Ok(observed) if observed == expected => fatal,
                Ok(observed) => StepFatal::Internal { expected, observed },
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
            Some(fatal) => {
                self.poison = Some(fatal.clone());
                Err(fatal)
            }
            None => Ok(()),
        }
    }
}
