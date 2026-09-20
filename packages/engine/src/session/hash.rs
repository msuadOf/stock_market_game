use super::{GameSession, StepFatal};
use serde::Serialize;

/// FNV-1a over length-delimited canonical JSON fields; not a security digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StateHash(u64);

/// Business and session projections captured before a discardable tick begins.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct RollbackHashes {
    pub(super) business: StateHash,
    pub(super) session: StateHash,
}

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
    pub(super) fn rollback_hashes(&self) -> Result<RollbackHashes, StepFatal> {
        let business = self.business_state_hash()?;
        let session = self.session_state_hash_from_business(business)?;
        Ok(RollbackHashes { business, session })
    }

    /// Includes every business field below, including unfiltered pending plan facts.
    /// Excludes poison, injection controls, retail diagnostic caches and feature collectors.
    /// Non-authoritative decision fixtures are rejected, never projected as production state.
    pub fn business_state_hash(&self) -> Result<StateHash, StepFatal> {
        let Self {
            setup: _,
            rng: _,
            seed: _,
            markets: _,
            accounts: _,
            price_history: _,
            market_minute_closes: _,
            daily_candles: _,
            active_daily_candles: _,
            auction_orders: _,
            auction_order_counts: _,
            pending_player: _,
            npc_attention: _,
            retail_experience: _,
            parent_orders: _,
            pending_plan_events: _,
            npc_order_lifecycles: _,
            attention_queue: _,
            company_registry: _,
            operations: _,
            closing: _,
            library: _,
            ops_wiring: _,
            disclosures: _,
            plans: _,
            information: _,
            belief_books: _,
            watchlists: _,
            price_memories: _,
            envelope_ledger: _,
            retail_projection_seen: _,
            next_receipt_base: _,
            next_order_id: _,
            tick: _,
            day: _,
            seq: _,
            civil_clock: _,
            poison: _,
            last_retail_decisions: _,
            last_retail_order_events: _,
            #[cfg(test)]
                injected_failure: _,
            #[cfg(test)]
                post_shadow_failure: _,
            #[cfg(feature = "simulation-diagnostics")]
                npc_decision_traces: _,
            #[cfg(feature = "simulation-diagnostics")]
                causal: _,
        } = self;
        let mut hash = StateHash(0xcbf29ce484222325);
        hash.field(&self.setup)?;
        hash.field(&self.seed)?;
        hash.field(&self.rng.state)?;
        for (id, account) in &self.accounts {
            let crate::account::Account {
                id: _,
                kind: _,
                cash: _,
                positions: _,
                strategy: _,
            } = account;
            hash.field(&(
                id,
                account.id,
                account.kind,
                account.cash,
                &account.positions,
            ))?;
            let state = account
                .strategy
                .as_ref()
                .map(|strategy| strategy.production_state())
                .transpose()
                .map_err(|error| StepFatal::InvariantViolation {
                    description: error.to_string(),
                    location: "state_hash.strategy".to_owned(),
                })?;
            hash.field(&state)?;
        }
        for (code, market) in &self.markets {
            hash.field(&(code, market.hash_projection()))?;
        }
        hash.field(&self.price_history)?;
        hash.field(&self.market_minute_closes)?;
        hash.field(&self.daily_candles)?;
        hash.field(&self.active_daily_candles)?;
        hash.field(&self.auction_orders)?;
        hash.field(&self.auction_order_counts)?;
        hash.field(&self.pending_player)?;
        hash.field(&self.npc_attention)?;
        hash.field(&self.retail_experience)?;
        hash.field(&self.parent_orders)?;
        hash.field(&self.pending_plan_events)?;
        hash.field(&self.npc_order_lifecycles)?;
        let mut queue: Vec<_> = self.attention_queue.iter().map(|entry| entry.0).collect();
        queue.sort_unstable();
        hash.field(&queue)?;
        hash.field(&self.company_registry)?;
        hash.field(&self.operations)?;
        hash.field(&self.closing)?;
        hash.field(&self.library)?;
        hash.field(&self.ops_wiring)?;
        hash.field(&self.disclosures)?;
        hash.field(&self.plans)?;
        hash.field(&self.information)?;
        hash.field(&self.belief_books)?;
        hash.field(&self.watchlists)?;
        hash.field(&self.price_memories)?;
        hash.field(&self.envelope_ledger.hash_projection())?;
        hash.field(&self.retail_projection_seen)?;
        hash.field(&(
            self.next_order_id,
            self.tick,
            self.day,
            self.seq,
            self.next_receipt_base,
        ))?;
        hash.field(&self.civil_clock.save())?;
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
        for trace in &self.last_retail_decisions {
            hash.field(&(
                trace.account,
                &trace.decision.code,
                format!("{:?}", trace.decision.action),
                format!("{:?}", trace.decision.reason),
                trace.decision.target_position_fraction.to_bits(),
                trace.decision.desired_delta_shares,
                trace.decision.executable_delta_shares,
            ))?;
        }
        hash.field(&self.last_retail_order_events)?;
        #[cfg(feature = "simulation-diagnostics")]
        {
            hash.field(&self.npc_decision_traces)?;
            hash.field(&self.causal)?;
        }
        Ok(hash)
    }
}
