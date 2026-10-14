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
        self.state.library.publication_ids()
    }
    pub const fn poison_reason(&self) -> Option<&StepFatal> {
        self.poison.as_ref()
    }

    pub fn require_healthy(&self) -> Result<(), StepFatal> {
        match &self.poison {
            Some(fatal) => Err(fatal.clone()),
            None => Ok(()),
        }
    }

    /// 所有市场工作运行在私有 tick shadow；失败候选被丢弃。
    /// 权威状态只接收最终不会失败的 CommitTick 状态交换。
    ///
    /// 返回的事件数组是从已提交事实记录批次投影出的交付视图（架构项 #6）：
    /// 会话内部不保留第二份事件权威，身份仍只存在于 `EventFact` 记录中。
    pub fn step(&mut self) -> Result<Vec<Event>, StepFatal> {
        self.step_inner(false)
            .map(|committed| committed.facts.into_iter().map(|fact| fact.event).collect())
    }

    /// 执行与 [`Self::step`] 相同的生产权威路径，并返回
    /// 保留成功提交前一刻捕获的不可变执行事实。
    ///
    /// 此验证入口只用于观察；证据不会留在 session、序列化、参与 hash 或后续决策。
    pub fn step_with_commit_evidence(
        &mut self,
    ) -> Result<(Vec<Event>, super::pipeline::TickCommitEvidence), StepFatal> {
        let committed = self.step_inner(true)?;
        let evidence = committed
            .evidence
            .expect("commit evidence was requested at the authoritative entry");
        Ok((
            committed.facts.into_iter().map(|fact| fact.event).collect(),
            evidence,
        ))
    }

    pub(super) fn step_inner(
        &mut self,
        capture_commit_evidence: bool,
    ) -> Result<super::pipeline::AuthoritativeTickCommit, StepFatal> {
        self.step_on_context(capture_commit_evidence)
    }

    /// 协调逻辑在调用线程；各并行 burst 经会话上下文进入其池（架构项 14）。
    pub(crate) fn step_on_context(
        &mut self,
        capture_commit_evidence: bool,
    ) -> Result<super::pipeline::AuthoritativeTickCommit, StepFatal> {
        self.check_step_start()
            .map_err(|fatal| self.poison_failed_step(fatal))?;
        // 每个生产交易阶段均进入同一 Escrow 完整 tick 事务。
        // 只有完成不会失败的权威状态交换后才交付结果。
        let result = super::pipeline::execute_authoritative_tick(self, capture_commit_evidence);
        match result {
            Ok(committed) => Ok(committed),
            Err(fatal) => Err(self.poison_failed_step(fatal)),
        }
    }

    /// step 家族共用的前置检查：健康状态、测试注入失败与 NPC 策略完整性。
    /// frame 投影路径（step_frame_and_commit）与 step_inner 都从这里进入同一 dispatcher。
    pub(super) fn check_step_start(&mut self) -> Result<(), StepFatal> {
        self.require_healthy()?;
        #[cfg(test)]
        self.run_pre_mutation_hook()?;
        if self.state.accounts.has_missing_npc_strategy() {
            return Err(StepFatal::InvariantViolation {
                description: "non-player account has no authoritative strategy".to_owned(),
                location: "GameSession::step".to_owned(),
            });
        }
        Ok(())
    }

    /// runtime 驱动器翻转面（批 8h）：tick 前进（经直改标记）。
    pub fn set_runtime_tick(&mut self, tick: u64) {
        self.mark_external_mutation();
        self.state.tick = tick;
    }

    /// runtime 驱动器翻转面（批 8h）：事件权威 seq 写回。
    pub fn set_runtime_seq(&mut self, seq: u64) {
        self.mark_external_mutation();
        self.state.seq = seq;
    }

    /// 当前事件权威 seq（驱动器游标初始化）。
    pub fn current_seq(&self) -> u64 {
        self.state.seq
    }


    /// runtime 驱动器翻转面（批 8h）：披露 observer 注册透传。
    pub fn add_disclosure_observer_for_runtime(
        &mut self,
        observer: fn(crate::calendar::CivilInstant),
    ) {
        self.state.civil_clock.add_disclosure_observer(observer);
    }

    /// runtime 驱动器翻转面（批 8h）：行情直改入口（X1 标记）。
    pub fn market_mut_for_runtime(
        &mut self,
        code: &crate::StockId,
    ) -> Option<&mut crate::Market> {
        self.state.markets.get_mut(code)
    }

    /// runtime 驱动器翻转面（批 8 终点）：历史 K 回写。
    pub fn push_historical_candle_for_runtime(
        &mut self,
        code: &crate::StockId,
        candle: crate::session::DailyCandle,
    ) {
        self.mark_external_mutation();
        self.state.candle_book.push_history(code, candle);
    }

    /// runtime 驱动器翻转面（批 8 终点）：活动 K 安装。
    pub fn set_active_candle_for_runtime(
        &mut self,
        code: &crate::StockId,
        candle: crate::session::DailyCandle,
    ) {
        self.mark_external_mutation();
        self.state.candle_book.install_active(code.clone(), candle);
    }

    /// runtime 驱动器翻转面（批 8h）：失败 poison（旧管线语义）。
    pub fn poison_runtime_step(&mut self, fatal: StepFatal) -> StepFatal {
        self.poison_failed_step(fatal)
    }


    pub(super) fn poison_failed_step(&mut self, fatal: StepFatal) -> StepFatal {
        self.poison = Some(fatal.clone());
        fatal
    }

    pub fn save(&self) -> Result<SaveSlot, StepFatal> {
        let mut save = self.save_committed_projection()?;
        self.project_shared_ingress_save(&mut save)?;
        Ok(save)
    }

    pub(in crate::session) fn save_committed_projection(&self) -> Result<SaveSlot, StepFatal> {
        self.require_healthy()?;
        let runtime_state = super::persistence::capture_runtime_state(self)?;
        Ok(self.save_projection(runtime_state))
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
