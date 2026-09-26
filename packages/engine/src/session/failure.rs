use super::{Event, GameSession, SaveSlot};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, thiserror::Error)]
pub enum StepFatal {
    #[error("invariant violation at {location}: {description}")]
    InvariantViolation {
        description: String,
        location: String,
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

    /// All market work runs on a private tick shadow. A failed candidate is discarded;
    /// the authority receives only the final infallible P9 swap.
    pub fn step(&mut self) -> Result<Vec<Event>, StepFatal> {
        self.step_inner(false).map(|committed| committed.events)
    }

    /// Executes the same production authority path as [`Self::step`] and returns
    /// the immutable facts captured immediately before its successful P9 swap.
    ///
    /// This verification seam is observational: the evidence is not retained in
    /// the session, serialized, hashed, or made visible to later decisions.
    pub fn step_with_commit_evidence(
        &mut self,
    ) -> Result<(Vec<Event>, super::pipeline::TickCommitEvidence), StepFatal> {
        let committed = self.step_inner(true)?;
        Ok((
            committed.events,
            committed
                .evidence
                .expect("commit evidence was requested at the authoritative entry"),
        ))
    }

    pub(super) fn step_inner(
        &mut self,
        capture_commit_evidence: bool,
    ) -> Result<super::pipeline::AuthoritativeTickCommit, StepFatal> {
        self.require_healthy()?;
        #[cfg(test)]
        if self.injected_failure.is_some() {
            if let Err(fatal) = self.run_pre_mutation_hook() {
                return Err(self.poison_failed_step(fatal));
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
            return Err(self.poison_failed_step(fatal));
        }
        // Every production phase enters one escrow-backed P0-P9 transaction.
        // The phase dispatcher returns only after the prepared candidate has
        // completed its infallible P9 authority swap.
        let result = super::pipeline::execute_authoritative_tick(self, capture_commit_evidence);
        match result {
            Ok(committed) => Ok(committed),
            Err(fatal) => Err(self.poison_failed_step(fatal)),
        }
    }

    pub(super) fn poison_failed_step(&mut self, fatal: StepFatal) -> StepFatal {
        self.poison = Some(fatal.clone());
        fatal
    }

    pub fn save(&self) -> Result<SaveSlot, StepFatal> {
        self.require_healthy()?;
        let runtime_v2 = super::persistence::capture_runtime_v2(self)?;
        Ok(self.save_projection(runtime_v2))
    }

    #[cfg(test)]
    pub(super) fn inject_step_failure(&mut self, fatal: StepFatal) {
        self.injected_failure = Some(fatal);
    }

    #[cfg(test)]
    pub(crate) fn inject_post_shadow_failure(&mut self, fatal: StepFatal) {
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
