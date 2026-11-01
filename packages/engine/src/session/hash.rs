use super::{GameSession, StepFatal};
use serde::Serialize;
#[cfg(test)]
use std::cell::RefCell;
use std::collections::BTreeMap;

/// FNV-1a over length-delimited canonical JSON fields; not a security digest.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct StateHash(u64);

#[cfg(test)]
thread_local! {
    static BUSINESS_HASH_TRACE: RefCell<Option<Vec<Vec<u8>>>> = const { RefCell::new(None) };
}

#[cfg(test)]
struct BusinessHashTraceGuard;

#[cfg(test)]
impl BusinessHashTraceGuard {
    fn begin() -> Self {
        BUSINESS_HASH_TRACE.with(|trace| {
            let mut trace = trace.borrow_mut();
            assert!(trace.is_none(), "business hash trace不允许嵌套");
            *trace = Some(Vec::new());
        });
        Self
    }

    fn finish(self) -> Vec<Vec<u8>> {
        let fields = BUSINESS_HASH_TRACE.with(|trace| {
            trace
                .borrow_mut()
                .take()
                .expect("business hash trace生命周期已损坏")
        });
        fields
    }
}

#[cfg(test)]
impl Drop for BusinessHashTraceGuard {
    fn drop(&mut self) {
        BUSINESS_HASH_TRACE.with(|trace| *trace.borrow_mut() = None);
    }
}

#[cfg(test)]
pub(in crate::session) fn capture_business_hash_for_test(
    session: &GameSession,
) -> Result<(StateHash, Vec<Vec<u8>>), StepFatal> {
    let guard = BusinessHashTraceGuard::begin();
    let hash = session.business_state_hash()?;
    let fields = guard.finish();
    Ok((hash, fields))
}

impl StateHash {
    fn field(&mut self, value: &impl Serialize) -> Result<(), StepFatal> {
        let bytes = serde_json::to_vec(value).map_err(|error| StepFatal::InvariantViolation {
            description: error.to_string(),
            location: "state_hash.serialization".to_owned(),
        })?;
        #[cfg(test)]
        BUSINESS_HASH_TRACE.with(|trace| {
            if let Some(fields) = trace.borrow_mut().as_mut() {
                fields.push(bytes.clone());
            }
        });
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
            market_calendar_source: _,
            market_calendar_day: _,
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
            runtime_authority_epoch: _,
            runtime_authority_identity: _,
            runtime_external_authority: _,
            runtime_installed_market_close: _,
            serial_committed_market_close: _,
            runtime_mirror_install_sequence: _,
            runtime_mirror_origin: _,
            runtime_publication_generation: _,
            runtime_discard_driver_flag: _,
            revision: _,
            report_correction_epoch: _,
        } = self;
        let super::CommittableSessionState {
            setup: _,
            memberships: _,
            validated_config: _,
            stocks: _,
            rng: _,
            seed: _,
            markets: _,
            accounts: _,
            history_reads: _,
            price_history: _,
            market_minute_closes: _,
            market_minute_bars: _,
            retained_market_days: _,
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
            runtime_heap_dirty: _,
            runtime_resting_snapshot: _,
            runtime_account_undo: _,
            runtime_account_undo_ids: _,
            runtime_attention_undo: _,
            runtime_attention_undo_ids: _,
            runtime_heap_undo: _,
            runtime_belief_undo: _,
            runtime_belief_undo_ids: _,
            runtime_retail_undo: _,
            runtime_retail_undo_ids: _,
            runtime_receipts_undo: _,
            runtime_receipts_undo_ids: _,
            runtime_history_undo: _,
            runtime_history_undo_ids: _,
            runtime_mirror_undo: _,
            runtime_plans_undo: _,
            runtime_envelope_undo: _,
            runtime_lifecycles_undo: _,
            runtime_retail_diag_undo: _,
            companies: _,
            report_corrections: _,
            library: _,
            disclosures: _,
            plans: _,
            urgency_policy: _,
            // Sol 六审段 1.3：运行时权威标志（非业务事实——不参与哈希）。
            runtime_information_authority: _,
            belief_participants: _,
            envelope_ledger: _,
            retail_projection_seen: _,
            next_receipt_base: _,
            personal_trade_confirmations: _,
            receipt_watermark: _,
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
        // P2-1（视图内生战役）：heap 惰性化后 hash 从候选权威派生（等价
        // 论证：heap=每账户恰一项 (候选 tick, id) 的权威快照——集合相同，
        // 排序后同序列 → hash 同值；改派生后 hash 不再依赖镜像重建时点）。
        let mut queue: Vec<_> = self
            .state
            .npc_attention
            .iter()
            .map(|(id, state)| (state.next_attention_candidate_tick, *id))
            .collect();
        queue.sort_unstable();
        hash.field(&queue)?;
        match &self.state.companies {
            super::company_state::SessionCompanies::LegacyOperations(legacy) => {
                hash.field(&0_u8)?;
                hash.field(legacy.registry.as_ref())?;
                hash.field(&legacy.operations.hash_projection().map_err(|error| {
                    StepFatal::InvariantViolation {
                        description: error.to_string(),
                        location: "state_hash.company_operations".to_owned(),
                    }
                })?)?;
                hash.field(&legacy.closing.hash_projection().map_err(|error| {
                    StepFatal::InvariantViolation {
                        description: error.to_string(),
                        location: "state_hash.closing".to_owned(),
                    }
                })?)?;
                hash.field(&legacy.groups)?;
            }
            super::company_state::SessionCompanies::CompanySystem(system) => {
                hash.field(&1_u8)?;
                hash.field(&system.system.hash_projection().map_err(|error| {
                    StepFatal::InvariantViolation {
                        description: error.to_string(),
                        location: "state_hash.company_system".to_owned(),
                    }
                })?)?;
                hash.field(&system.corporate_actions)?;
            }
        }
        hash.field(&self.state.report_corrections)?;
        hash.field(&self.state.library.hash_projection())?;
        hash.field(&self.state.disclosures)?;
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
        // P2-4：ledger 每拍维护退役——hash 从派生投影（快照→envelopes→
        // 同构临时 ledger 的 hash_projection；构造与原全量重建同一函数，
        // 内容等价由同源性承载）。
        // Sol 复核阻断 P2-4：无快照（旧管线维护中/restore 未步进/shadow
        // 边界）读**实际 ledger**——统一"有快照派生、无快照读实际
        // ledger"合同（对齐 save 面 saved_runtime 的分支先例）；无条件
        // 派生使非空 ledger 漏出 hash。
        if self.state.runtime_resting_snapshot.is_some() {
            let derived_envelopes = self.runtime_derive_envelopes()?;
            let derived_ledger = crate::session::pipeline::EnvelopeLedger::new(
                self.state.next_receipt_base,
                derived_envelopes,
            )
            .map_err(|fatal| StepFatal::InvariantViolation {
                description: format!("派生信封账本构建失败：{fatal:?}"),
                location: "state_hash.envelope_derived".to_owned(),
            })?;
            hash.field(&derived_ledger.hash_projection())?;
        } else {
            hash.field(&self.state.envelope_ledger.hash_projection())?;
        }
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
