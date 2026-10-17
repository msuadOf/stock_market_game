use super::{GameSession, StepFatal};
use serde::Serialize;
use std::collections::BTreeMap;

/// FNV-1a over length-delimited canonical JSON fields; not a security digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StateHash(u64);

impl StateHash {
    fn field(&mut self, value: &impl Serialize) -> Result<(), StepFatal> {
        let bytes = serde_json::to_vec(value).map_err(|error| StepFatal::InvariantViolation {
            description: error.to_string(),
            location: "state_hash.serialization".to_owned(),
        })?;
        let length = u64::try_from(bytes.len()).map_err(|error| StepFatal::InvariantViolation {
            description: error.to_string(),
            location: "state_hash.length".to_owned(),
        })?;
        for byte in length.to_le_bytes().into_iter().chain(bytes) {
            self.0 = (self.0 ^ u64::from(byte)).wrapping_mul(0x100000001b3);
        }
        Ok(())
    }
}

impl GameSession {
    /// Includes every business field below, including unfiltered pending plan facts.
    /// Excludes poison, injection controls, retail diagnostic caches and feature collectors.
    /// Non-authoritative decision fixtures are rejected, never projected as production state.
    pub fn business_state_hash(&self) -> Result<StateHash, StepFatal> {
        let Self {
            compute: _,
            ingress: _,
            state,
            poison: _,
            fresh_initial_allocation: _,
            #[cfg(test)]
                injected_failure: _,
            #[cfg(test)]
                post_shadow_failure: _,
            #[cfg(test)]
                runtime_install_failure: _,
            runtime_driver: _,
            runtime_group_count: _,
            runtime_discard_driver_flag: _,
            revision: _,
        } = self;
        let super::CommittableSessionState {
            setup: _,
            validated_config: _,
            stocks: _,
            rng: _,
            seed: _,
            markets: _,
            accounts: _,
            history_reads: _,
            price_history: _,
            market_minute_closes: _,
            candle_book: _,
            pending_player: _,
            ingress_receipt_cursors: _,
            npc_attention: _,
            desk_book_mirror: _,
            runtime_desk_serial_cursors: _,
            retail_experience: _,
            npc_order_lifecycles: _,
            last_retail_decisions: _,
            last_retail_order_events: _,
            #[cfg(feature = "simulation-diagnostics")]
                npc_decision_traces: _,
            #[cfg(feature = "simulation-diagnostics")]
                causal: _,
            attention_scheduler: _,
            runtime_account_undo: _,
            runtime_account_undo_ids: _,
            runtime_attention_undo: _,
            runtime_attention_undo_ids: _,
            runtime_heap_undo: _,
            runtime_belief_undo: _,
            runtime_belief_undo_ids: _,
            runtime_retail_undo: _,
            runtime_retail_undo_ids: _,
            runtime_history_undo: _,
            runtime_history_undo_ids: _,
            runtime_mirror_undo: _,
            runtime_plans_undo: _,
            runtime_envelope_undo: _,
            runtime_lifecycles_undo: _,
            runtime_retail_diag_undo: _,
            company_registry: _,
            operations: _,
            closing: _,
            library: _,
            disclosures: _,
            groups: _,
            plans: _,
            urgency_policy: _,
            belief_participants: _,
            envelope_ledger: _,
            retail_projection_seen: _,
            next_receipt_base: _,
            next_order_id: _,
            tick: _,
            day: _,
            seq: _,
            civil_clock: _,
        } = state;
        let mut hash = StateHash(0xcbf29ce484222325);
        hash.field(&self.state.setup)?;
        hash.field(&self.state.seed)?;
        hash.field(&self.state.rng.state)?;
        for (id, account) in &self.state.accounts {
            hash.field(&(
                id,
                account.id(),
                account.kind(),
                account.cash(),
                &account.positions(),
            ))?;
            let state = account
                .strategy()
                .map(|strategy| strategy.production_state())
                .transpose()
                .map_err(|error| StepFatal::InvariantViolation {
                    description: error.to_string(),
                    location: "state_hash.strategy".to_owned(),
                })?;
            hash.field(&state)?;
        }
        for (code, market) in &self.state.markets {
            hash.field(&(code, market.hash_projection()))?;
        }
        hash.field(&self.state.price_history)?;
        hash.field(&self.state.market_minute_closes)?;
        hash.field(&self.state.candle_book.histories())?;
        hash.field(&self.state.candle_book.active())?;
        hash.field(&self.state.pending_player)?;
        hash.field(&self.state.ingress_receipt_cursors)?;
        hash.field(&self.state.npc_attention)?;
        hash.field(&self.state.history_reads)?;
        hash.field(&self.state.retail_experience)?;
        hash.field(&self.state.npc_order_lifecycles)?;
        let mut queue: Vec<_> = self.state.attention_scheduler.iter().copied().collect();
        queue.sort_unstable();
        hash.field(&queue)?;
        hash.field(self.state.company_registry.as_ref())?;
        let operations = self.state.operations.hash_projection().map_err(|error| {
            StepFatal::InvariantViolation {
                description: error.to_string(),
                location: "state_hash.company_operations".to_owned(),
            }
        })?;
        hash.field(&operations)?;
        let closing = self.state.closing.hash_projection().map_err(|error| {
            StepFatal::InvariantViolation {
                description: error.to_string(),
                location: "state_hash.closing".to_owned(),
            }
        })?;
        hash.field(&closing)?;
        hash.field(&self.state.library.hash_projection())?;
        hash.field(&self.state.disclosures)?;
        hash.field(&self.state.groups)?;
        hash.field(&self.state.plans)?;
        hash.field(&self.state.urgency_policy)?;
        hash.field(
            &self
                .state
                .belief_participants
                .iter()
                .map(|(id, participant)| (id, participant.information()))
                .collect::<BTreeMap<_, _>>(),
        )?;
        hash.field(
            &self
                .state
                .belief_participants
                .iter()
                .map(|(id, participant)| (id, participant.belief()))
                .collect::<BTreeMap<_, _>>(),
        )?;
        hash.field(
            &self
                .state
                .belief_participants
                .iter()
                .map(|(id, participant)| (id, participant.watchlist()))
                .collect::<BTreeMap<_, _>>(),
        )?;
        hash.field(
            &self
                .state
                .belief_participants
                .iter()
                .map(|(id, participant)| (id, participant.price_memory()))
                .collect::<BTreeMap<_, _>>(),
        )?;
        hash.field(&self.state.envelope_ledger.hash_projection())?;
        hash.field(&self.state.retail_projection_seen)?;
        hash.field(&(
            self.state.next_order_id,
            self.state.tick,
            self.state.day,
            self.state.seq,
            self.state.next_receipt_base,
        ))?;
        hash.field(&self.state.civil_clock.save())?;
        Ok(hash)
    }

    /// Business projection plus poison and diagnostic caches. Function-pointer observers
    /// and test injection controls are execution configuration, not serialized state.
    pub fn session_state_hash(&self) -> Result<StateHash, StepFatal> {
        self.session_state_hash_from_business(self.business_state_hash()?)
    }

    fn session_state_hash_from_business(
        &self,
        mut hash: StateHash,
    ) -> Result<StateHash, StepFatal> {
        hash.field(&self.poison)?;
        for trace in &self.state.last_retail_decisions {
            hash.field(&(
                trace.account,
                &trace.decision.code,
                format!("{:?}", trace.decision.action),
                format!("{:?}", trace.decision.reason),
                trace.decision.target_position_fraction.to_bits(),
                trace.decision.desired_delta_shares,
                trace.decision.executable_delta_shares,
                format!("{:?}", trace.execution_urgency),
            ))?;
        }
        hash.field(&self.state.last_retail_order_events)?;
        #[cfg(feature = "simulation-diagnostics")]
        {
            hash.field(&self.state.npc_decision_traces)?;
            hash.field(&self.state.causal)?;
        }
        Ok(hash)
    }
}
