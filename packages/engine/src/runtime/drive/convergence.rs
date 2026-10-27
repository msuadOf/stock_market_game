use super::*;
use std::collections::BTreeMap;
use std::sync::Arc;

/// World 已发布后的只读投影输入；构造它不写 Session 镜像或消费完整读取。
pub(crate) struct PreparedDriverStep {
    pub calendar: std::sync::Arc<crate::runtime::MarketTickContext>,
    pub accounts: Vec<
        Arc<
            Vec<(
                super::super::LocalIdx,
                Arc<super::super::group::GroupAccount>,
            )>,
        >,
    >,
    pub buy_vwap: BTreeMap<
        (
            super::super::GroupIdx,
            super::super::LocalIdx,
            super::super::StockIdx,
        ),
        i64,
    >,
    pub last_prices: Vec<i64>,
    pub personals: Vec<(
        usize,
        AccountId,
        super::super::world::BeliefParticipantSnapshot,
    )>,
    pub plans: crate::plans::PlanBook,
    pub touched: Vec<(usize, AccountId)>,
    pub events: Vec<Event>,
}

impl RuntimeDriver {
    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn retain_terminal_causal(
        &mut self,
        time: crate::diagnostics::causal::FactTime,
        raw: Vec<crate::runtime::diagnostics::DeskCausalFact>,
        fatal: crate::session::StepFatal,
    ) {
        if self.terminal_causal.is_none() {
            self.terminal_causal = Some((time, raw, fatal));
        }
    }

    #[cfg(feature = "simulation-diagnostics")]
    pub(crate) fn retain_pending_causal_failure(
        &mut self,
        time: crate::diagnostics::causal::FactTime,
        fatal: crate::session::StepFatal,
    ) {
        let raw = self.take_causal_facts();
        self.retain_terminal_causal(time, raw, fatal);
    }

    #[cfg(all(test, feature = "simulation-diagnostics"))]
    pub(crate) fn terminal_causal_snapshot_for_test(&self) -> Option<(
        crate::diagnostics::causal::FactTime,
        Vec<crate::runtime::diagnostics::DeskCausalFact>,
        crate::session::StepFatal,
    )> {
        self.terminal_causal.clone()
    }

    pub(crate) fn cached_personal_at(
        &self,
        index: usize,
    ) -> Option<&super::super::world::BeliefParticipantSnapshot> {
        self.belief_cache.get(index).and_then(Option::as_ref)
    }

    pub(crate) fn install_cached_personal_at(
        &mut self,
        index: usize,
        account: AccountId,
        value: super::super::world::BeliefParticipantSnapshot,
    ) {
        assert_eq!(
            self.id_map[index], account,
            "candidate 已验证个人态真实名册"
        );
        self.belief_cache[index] = Some(value);
        #[cfg(test)]
        {
            self.personal_installs += 1;
        }
    }

    pub(crate) fn finish_mirror_install(&mut self, accounts: usize) {
        #[cfg(test)]
        {
            self.account_writes += accounts as u64;
        }
        #[cfg(not(test))]
        let _ = accounts;
        self.force_full_accounts = false;
        self.pending_receipts.clear();
        self.pending_hold_inits.clear();
    }

    pub(crate) fn pending_trade_receipts(
        &self,
    ) -> &[crate::session::personal_trade_history::PendingTradeReceipt] {
        &self.pending_receipts
    }

    /// 兼容直接 driver 调用；生产聚合入口在全部域准备成功后才调用安装。
    pub(super) fn install_driver_projection(
        &mut self,
        session: &mut GameSession,
        prepared: PreparedDriverStep,
    ) -> Result<Vec<Event>, String> {
        self.install_account_projection(
            session,
            prepared
                .accounts
                .iter()
                .enumerate()
                .flat_map(|(group, rows)| {
                    rows.iter()
                        .map(move |(local, account)| (group, *local, account.as_ref()))
                }),
            &prepared.buy_vwap,
            &prepared.last_prices,
        );
        for (global, id, snapshot) in prepared.personals {
            session.set_runtime_personal_state(id, &snapshot);
            self.belief_cache[global] = Some(snapshot);
            #[cfg(test)]
            {
                self.personal_installs += 1;
            }
        }
        self.install_pending_hold_inits(session, None)?;
        session.set_runtime_plans(prepared.plans);
        for (_, account) in prepared.touched {
            session.runtime_prune_personal_memory(account);
        }
        self.force_full_accounts = false;
        Ok(prepared.events)
    }

    pub(crate) fn prepare_driver_step(
        &mut self,
        session: &GameSession,
    ) -> Result<PreparedDriverStep, String> {
        self.world.set_civil_date(session.civil_date());
        self.world
            .set_tick_capture(crate::verification_evidence::current_phase_timing_binding());
        let result = self.prepare_step_with_capture(session);
        self.world.clear_tick_capture();
        result
    }
}

impl RuntimeDriver {
    #[cfg(test)]
    pub(crate) fn record_root_handover(session: &GameSession, tick_after: u64, has_events: bool) {
        let probes = &session.compute_context().t1_probes;
        let handover_seq = probes
            .root_handover_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        // 交付槽消费（同计数域：完成=奇数位、交付=紧邻偶数位
        // ——旧管线交付侧 fetch；runtime 交付即事件返回本身，
        // 槽位仍须成对推进以保奇偶语义）。
        let _delivery_slot = probes
            .root_handover_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;
        let attempt = probes
            .queue_attempt_seq
            .load(std::sync::atomic::Ordering::SeqCst);
        let roots = usize::from(has_events);
        let mut trace = probes
            .root_handover_trace
            .lock()
            .expect("root 交接见证锁被 poison");
        let capture_ordinal = trace.iter().filter(|r| r.tick == tick_after).count() as u32 + 1;
        let backtrace = if capture_ordinal >= 2 && trace.iter().all(|r| r.backtrace.is_none()) {
            Some(format!("{:?}", std::backtrace::Backtrace::force_capture()))
        } else {
            None
        };
        trace.push(crate::compute::RootHandoverRecord {
            // 旧口径：记录 tick=消费 tick（步进前值——交接
            // 发生在被消费 tick 的执行相位）。
            tick: tick_after - 1,
            attempt,
            roots,
            handover_seq,
            capture_ordinal,
            backtrace,
        });
    }
}
