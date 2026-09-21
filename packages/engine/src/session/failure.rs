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
        #[cfg(test)]
        let has_non_authoritative_test_strategy = self.accounts.values().any(|account| {
            account
                .strategy
                .as_ref()
                .is_some_and(|strategy| !strategy.is_production())
        });
        #[cfg(not(test))]
        let has_non_authoritative_test_strategy = false;

        // Capture the compact rollback projection once. Public information contributes its
        // incrementally maintained content digest, so this remains content-sensitive without
        // cloning or serializing the multi-megabyte publication corpus on every healthy tick.
        let rollback_before = if has_non_authoritative_test_strategy {
            None
        } else {
            match self.rollback_hashes() {
                Ok(hashes) => Some(hashes),
                Err(fatal) => {
                    self.poison = Some(fatal.clone());
                    return Err(fatal);
                }
            }
        };
        #[cfg(test)]
        if self.injected_failure.is_some() {
            if let Err(fatal) = self.run_pre_mutation_hook() {
                return Err(self.poison_failed_step(rollback_before, fatal));
            }
        }
        if self
            .accounts
            .values()
            .any(|account| account.kind != crate::AccountKind::Player && account.strategy.is_none())
        {
            let fatal = StepFatal::InvariantViolation {
                description: "non-player account has no authoritative strategy".to_owned(),
                location: "GameSession::step".to_owned(),
            };
            return Err(self.poison_failed_step(rollback_before, fatal));
        }
        #[cfg(test)]
        if rollback_before.is_none() {
            // Legacy test-only Strategy trait doubles cannot be cloned into the authoritative
            // StrategyState shadow. Preserve their narrow unit-test execution seam without making
            // the compatibility engine reachable from any production build.
            return Ok(self.step_current_behavior(false));
        }
        // Every production phase enters one escrow-backed P0-P9 transaction.
        // The phase dispatcher returns only after the prepared candidate has
        // completed its infallible P9 authority swap.
        let result = super::pipeline::execute_authoritative_tick(
            self,
            rollback_before.expect("production step captured rollback hashes"),
        );
        match result {
            Ok(events) => Ok(events),
            Err(fatal) => Err(self.poison_failed_step(rollback_before, fatal)),
        }
    }

    #[cfg(test)]
    pub(super) fn poison_failed_step_from_witness(
        &mut self,
        rollback_witness: Option<GameSession>,
        fatal: StepFatal,
    ) -> StepFatal {
        let rollback_before = match rollback_witness {
            Some(witness) => match witness.rollback_hashes() {
                Ok(hashes) => Some(hashes),
                Err(hash_error) => {
                    self.poison = Some(hash_error.clone());
                    return hash_error;
                }
            },
            None => None,
        };
        self.poison_failed_step(rollback_before, fatal)
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
        let runtime_v2 = super::persistence::capture_runtime_v2(self)?;
        Ok(self.save_projection(runtime_v2))
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
