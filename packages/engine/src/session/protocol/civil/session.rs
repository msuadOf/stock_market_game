use super::{CivilUpdate, TickBatch, TickFrame};
use crate::error::{ymd_context, ValidationFault};
use crate::session::{GameSession, SessionError, SessionSetup, StepFatal};
use crate::shared_history::AppendOnlyHistory;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

/// 一个未发布宿主批次的共同回滚集合；GameSession 保留自身 tick authority。
struct ProtocolState {
    game: GameSession,
    // 回滚点只复制 chunk 中的 Arc，已提交 frame 的 facts/payload 保持不可变共享。
    intraday: AppendOnlyHistory<Arc<TickFrame>>,
    fact_cursor: crate::session::protocol::SessionFactCursor,
    day_end_save: Option<Arc<crate::SaveSlot>>,
    committed_runtime: Option<super::super::PublicRuntimeState>,
    published_runtime: RefCell<Option<super::super::PublicRuntimeState>>,
    pending_save_candidates: RefCell<BTreeMap<u64, Arc<crate::SaveSlot>>>,
}

impl ProtocolState {
    fn try_clone_for_checkpoint(&self) -> Result<Self, StepFatal> {
        self.game.require_healthy()?;
        Ok(Self {
            game: self.game.clone_for_tick_shadow()?,
            intraday: self.intraday.clone(),
            fact_cursor: self.fact_cursor.clone(),
            day_end_save: self.day_end_save.clone(),
            committed_runtime: self.committed_runtime.clone(),
            published_runtime: RefCell::new(self.published_runtime.borrow().clone()),
            pending_save_candidates: RefCell::new(self.pending_save_candidates.borrow().clone()),
        })
    }
}

impl ProtocolSession {
    /// 显式转发 Simple 偏好配置，保留上一次已发布日终存档的职责。
    pub fn company_simple_preferences(
        &self,
        company: &crate::company::CompanyId,
    ) -> Result<
        crate::company::simple::SimpleCompanyPreferences,
        crate::session::SimplePreferencesError,
    > {
        self.state.game.company_simple_preferences(company)
    }

    pub fn set_simple_preferences(
        &mut self,
        company: &crate::company::CompanyId,
        preferences: crate::company::simple::SimpleCompanyPreferences,
    ) -> Result<(), crate::session::SimplePreferencesError> {
        self.state.game.set_simple_preferences(company, preferences)
    }

    /// B3 远程登录：成员变更面转发（bind/join/set_admission_cash 是会话级
    /// 静点操作，宿主在命令处理边界调用； ProtocolSession 未暴露通用
    /// DerefMut——成员操作经显式转发保持发布状态封装）。
    pub fn bind_market_creator(
        &mut self,
        subject: crate::session::OpaqueSubjectId,
    ) -> Result<crate::session::MarketMembership, crate::session::MembershipError> {
        self.state.game.bind_market_creator(subject)
    }

    pub fn join_market(
        &mut self,
        subject: crate::session::OpaqueSubjectId,
        confirmed_rejoin: bool,
    ) -> Result<crate::session::MarketMembership, crate::session::MembershipError> {
        self.state.game.join_market(subject, confirmed_rejoin)
    }

    pub fn set_admission_cash(
        &mut self,
        cash: crate::Money,
    ) -> Result<(), crate::session::MembershipError> {
        self.state.game.set_admission_cash(cash)
    }
}

pub struct ProtocolSession {
    state: ProtocolState,
    #[cfg(test)]
    malformed_frame: bool,
    #[cfg(test)]
    malformed_civil: bool,
}

pub struct ProtocolCheckpoint {
    state: ProtocolState,
}

impl ProtocolSession {
    /// 消费实际 session owner；只报告本 session 的 Rust 停止事实。
    pub fn shutdown(
        self,
    ) -> Result<crate::session::SessionShutdownReport, crate::runtime::world::ShutdownFailure> {
        self.state.game.shutdown()
    }

    pub fn query_stock_history(
        &mut self,
        account: crate::AccountId,
        code: &crate::StockId,
    ) -> Result<crate::session::HistoricalStockData, SessionError> {
        self.state.game.query_stock_history(account, code)
    }

    /// B2 查询面透传（wasm 宿主用）：保留历史分页（记 history_reads 副作用）。
    pub fn query_market_history_for(
        &mut self,
        account: crate::AccountId,
        request: &crate::session::MarketHistoryRequest,
    ) -> Result<crate::session::MarketHistoryPage, SessionError> {
        self.state.game.query_market_history_for(account, request)
    }

    /// B2 查询面透传（wasm 宿主用）：当日分钟K（记 history_reads 副作用）。
    pub fn query_current_minute_history_for(
        &mut self,
        account: crate::AccountId,
        request: &crate::session::CurrentMinuteHistoryRequest,
    ) -> Result<crate::session::CurrentMinuteHistoryResponse, SessionError> {
        self.state
            .game
            .query_current_minute_history_for(account, request)
    }

    /// B2 查询面透传（wasm 宿主用）：个人交割历史分页（纯查询无副作用）。
    pub fn query_personal_trade_history(
        &mut self,
        account: crate::AccountId,
        request: crate::session::PersonalTradeHistoryRequest,
    ) -> Result<crate::session::PersonalTradeHistoryPage, SessionError> {
        self.state
            .game
            .query_personal_trade_history(account, request)
    }

    /// B2 补面透传（wasm 宿主用）：本人交割单游标分页（纯查询）。
    pub fn personal_trade_confirmations_page(
        &self,
        account: crate::AccountId,
        before_receipt: Option<u64>,
    ) -> Vec<crate::session::PersonalTradeConfirmation> {
        self.state
            .game
            .personal_trade_confirmations_page(account, before_receipt)
    }

    /// 保存未发布宿主批次的 live state 与共享历史；避免文件投影丢失运行时诊断。
    pub fn checkpoint(&self) -> Result<ProtocolCheckpoint, StepFatal> {
        Ok(ProtocolCheckpoint {
            state: self.state.try_clone_for_checkpoint()?,
        })
    }

    /// 仅允许健康 serial 的机械回放；checkpoint 不复制 live World，不能撤回 runtime 提交。
    pub fn rollback(&mut self, checkpoint: ProtocolCheckpoint) -> Result<(), StepFatal> {
        self.state.game.require_healthy()?;
        let has_runtime_authority = |game: &GameSession| {
            game.runtime_authority_epoch != 0
                || game.runtime_mirror_install_sequence != 0
                || game.runtime_driver.is_some()
                || game.has_current_external_runtime_authority()
        };
        if has_runtime_authority(&self.state.game) || has_runtime_authority(&checkpoint.state.game)
        {
            return Err(StepFatal::InvariantViolation {
                location: "ProtocolSession::rollback".to_owned(),
                description:
                    "checkpoint 不持有 live World，不能回退已经安装或仍在位的 runtime authority"
                        .to_owned(),
            });
        }
        self.state = checkpoint.state;
        Ok(())
    }

    /// serial checkpoint、候选重试与旧管线决策合同使用原 exact case 验证；沿父 case 的外部十秒进程树预算。
    #[cfg(all(test, not(target_family = "wasm")))]
    pub(in crate::session) fn run_serial_checkpoint_probe_for_test() {
        assert!(
            crate::runtime::step_runtime_enabled(),
            "只有 native 父 case 启动 serial probe"
        );
        let thread = std::thread::current();
        let name = thread.name().expect("libtest case 必须有完整名字");
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture", "--test-threads=1"])
            .env("ENGINE_STEP_RUNTIME", "0")
            .output()
            .expect("启动原 checkpoint 合同的 serial exact probe");
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success()
                && stdout.contains("running 1 test")
                && stdout.contains("1 passed; 0 failed"),
            "serial checkpoint probe 必须真实执行原唯一 case：{name}\n{stdout}\n{stderr}"
        );
    }

    #[cfg(feature = "verification-harness")]
    pub fn fork_for_verification(&self) -> Result<Self, StepFatal> {
        let mut state = self.state.try_clone_for_checkpoint()?;
        state.game.freeze_shared_ingress()?;
        state.game.ingress = None;
        state.game.shared_ingress();
        Ok(Self {
            state,
            #[cfg(test)]
            malformed_frame: false,
            #[cfg(test)]
            malformed_civil: false,
        })
    }

    pub fn civil_day_ready(&self) -> Result<bool, SessionError> {
        Ok(self.state.game.day()
            == self
                .state
                .game
                .civil_clock()
                .completed_trading_sessions_expected()?)
    }

    pub fn enqueue_player_intent(
        &mut self,
        player: crate::AccountId,
        intent: crate::Intent,
    ) -> Result<(), SessionError> {
        self.state.game.enqueue_player_intent(player, intent)
    }

    pub fn restore(slot: &crate::SaveSlot) -> Result<Self, SessionError> {
        // 批 9 A5（with_compute 注入面退役）：wasm 串行与生产统一走
        // shared_default 内部构造（C 桶保留面——wasm 恒旧管线串行）。
        let game =
            GameSession::restore_with_compute(slot, crate::ComputeContext::shared_default()?)?;
        let sessions_through_current_date =
            game.civil_clock().completed_trading_sessions_expected()?;
        let current_date_trades =
            game.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading;
        let sessions_through_settled_date = sessions_through_current_date
            .checked_sub(u32::from(current_date_trades))
            .ok_or_else(|| {
                // fault3012: 自然日时钟的已完成交易会话计数不合法（当前日内会话数超过时钟计数）。
                SessionError::InvalidSave(ValidationFault::bare(3012))
            })?;
        let completed = slot.civil_clock.settled_through.is_some()
            && slot.snapshot.tick.is_multiple_of(slot.setup.ticks_per_day)
            && game.day() == sessions_through_settled_date;
        if !completed {
            // fault3013: 公共日级档必须来自完整的自然日日终结算。
            return Err(SessionError::InvalidSave(ValidationFault::bare(3013)));
        }
        if slot
            .resting_orders
            .values()
            .any(|orders| !orders.is_empty())
            || !slot.runtime_state.live_envelopes.is_empty()
            || !slot.npc_order_lifecycles.is_empty()
            || slot.plans.plan_ids().any(|plan_id| {
                slot.plans
                    .entry(plan_id)
                    .expect("plan_ids always resolve")
                    .execution()
                    .is_some()
            })
            || !slot.pending_player.is_empty()
        // 批四：pending_npc 载体删除（旧档在解码边界 fault2100 拒绝）。
        {
            // fault3014: 公共日级档不能包含日内活动委托或待处理输入。
            return Err(SessionError::InvalidSave(ValidationFault::bare(3014)));
        }
        Ok(Self::from_game(game, Some(Arc::new(slot.clone()))))
    }

    pub fn new(setup: SessionSetup, seed: u64) -> Result<Self, SessionError> {
        // 批 9 A5：同 restore——注入面退役，内部 shared_default。
        Ok(Self::from_game(
            GameSession::new_with_compute(setup, seed, crate::ComputeContext::shared_default()?)?,
            None,
        ))
    }

    /// Restore a validated in-memory verification checkpoint, without making
    /// it available as a public day-end save. Public loading still uses `restore`.
    #[cfg(feature = "verification-harness")]
    pub fn restore_verification_checkpoint_with_compute(
        slot: &crate::SaveSlot,
        compute: Arc<crate::ComputeContext>,
    ) -> Result<Self, SessionError> {
        // Sol 收官再复核阻断 9：验证恢复沿 harness 预算池（与
        // from_game_for_verification 对偶——默认通道恒绑 shared_default
        // 使预算隔离在"构造→保存→恢复"链上丢失）。
        Ok(Self::from_game(
            GameSession::restore_with_compute(slot, compute)?,
            None,
        ))
    }

    #[cfg(feature = "verification-harness")]
    pub fn restore_verification_checkpoint(slot: &crate::SaveSlot) -> Result<Self, SessionError> {
        // 批 9 A5：注入面退役——验证恢复经内部 shared_default（C 桶
        // escrow 验证面的 ComputeContext 消费仍由 GameSession 内部
        // 承载）。
        Ok(Self::from_game(
            GameSession::restore_with_compute(slot, crate::ComputeContext::shared_default()?)?,
            None,
        ))
    }

    /// C 桶（escrow 验证面）注入通道：宿主提供已绑定指定 ComputeContext
    /// 的 GameSession 构造协议会话——验证 harness 的预算隔离经此保留
    ///（A5 退役的是 crate 外生产注入面；此面 cfg verification-harness）。
    #[cfg(feature = "verification-harness")]
    pub fn from_game_for_verification(
        game: GameSession,
        day_end_save: Option<Arc<crate::SaveSlot>>,
    ) -> Self {
        Self::from_game(game, day_end_save)
    }

    /// 仅供内存初始 checkpoint 测试使用；不代表可加载的公共日终档。
    #[cfg(test)]
    pub(in crate::session) fn restore_initial_checkpoint_for_test(
        slot: &crate::SaveSlot,
    ) -> Result<Self, SessionError> {
        let game = GameSession::restore(slot)?;
        Ok(Self::from_game(game, None))
    }

    fn from_game(mut game: GameSession, day_end_save: Option<Arc<crate::SaveSlot>>) -> Self {
        game.shared_ingress();
        Self {
            state: ProtocolState {
                game,
                intraday: AppendOnlyHistory::default(),
                fact_cursor: crate::session::protocol::SessionFactCursor::default(),
                day_end_save,
                committed_runtime: None,
                published_runtime: RefCell::new(None),
                pending_save_candidates: RefCell::new(BTreeMap::new()),
            },
            #[cfg(test)]
            malformed_frame: false,
            #[cfg(test)]
            malformed_civil: false,
        }
    }

    pub fn game(&self) -> &GameSession {
        &self.state.game
    }

    /// epoch 对齐 fixture（A3）专用：可变访问内部 GameSession（cfg(test)）。
    #[cfg(test)]
    pub(crate) fn game_mut(&mut self) -> &mut GameSession {
        &mut self.state.game
    }

    pub fn shared_ingress(&self) -> crate::SharedSessionIngress {
        self.state
            .game
            .ingress
            .as_ref()
            .expect("ProtocolSession initializes shared ingress")
            .source
            .clone()
    }

    pub fn save(&self) -> Result<crate::SaveSlot, SessionError> {
        self.state.game.require_healthy()?;
        self.state
            .day_end_save
            .as_ref()
            .map(|slot| slot.as_ref().clone())
            .ok_or_else(|| {
                // fault3015: 尚未完成首次自然日日结，没有可用日终存档。
                SessionError::InvalidSave(ValidationFault::bare(3015))
            })
    }

    /// 显式同步后返回当前 Snapshot；不构造市场 TickFrame。
    pub fn flush_now(&mut self) -> Result<crate::Snapshot, StepFatal> {
        self.state.game.flush_now()?;
        let snapshot = self.state.game.runtime_snapshot();
        let current = self
            .state
            .game
            .public_runtime_state()
            .map_err(|fatal| self.state.game.poison_runtime_step(fatal))?;
        self.state.committed_runtime = Some(current.clone());
        *self.state.published_runtime.borrow_mut() = Some(current);
        Ok(snapshot)
    }

    /// 只读选择指定自然日已经成功生成的日终候选。
    pub fn save_current_day(
        &self,
        date: crate::CivilDate,
    ) -> Result<crate::SaveSlot, SessionError> {
        self.state.game.require_healthy()?;
        self.state
            .day_end_save
            .as_ref()
            .filter(|slot| slot.civil_clock.settled_through == Some(date))
            .map(|slot| slot.as_ref().clone())
            .ok_or_else(|| {
                SessionError::InvalidSave(ValidationFault::bare(
                    crate::error::SAVE_TARGET_DAY_NOT_READY,
                ))
            })
    }

    pub fn save_candidate(
        &self,
        key: &super::super::SaveCandidateKey,
    ) -> Result<crate::SaveSlot, SessionError> {
        self.state.game.require_healthy()?;
        let mut pending = self.state.pending_save_candidates.borrow_mut();
        let candidate = pending
            .get(&key.seq)
            .or_else(|| {
                self.state
                    .day_end_save
                    .as_ref()
                    .filter(|slot| slot.snapshot.seq == key.seq)
            })
            .ok_or_else(|| {
                // fault3016: 日终候选已失效或不存在；ctx[0]=请求的自然日 yyyymmdd，ctx[1]=请求的 seq
                // （原文本 "日终候选 {settled_date} / {seq} 已失效或不存在"）。
                let (year, month, day) = key.settled_date.ymd();
                SessionError::InvalidSave(ValidationFault::new(
                    3016,
                    vec![ymd_context(year, month, day), key.seq],
                ))
            })?;
        if candidate.civil_clock.settled_through != Some(key.settled_date) {
            // fault3017: 日终候选自然日与请求不一致（候选档 settled_through ≠ SaveCandidateKey.settled_date）。
            return Err(SessionError::InvalidSave(ValidationFault::bare(3017)));
        }
        let captured = candidate.as_ref().clone();
        pending.remove(&key.seq);
        Ok(captured)
    }

    pub fn discard_pending_save_candidates(&self) {
        self.state.pending_save_candidates.borrow_mut().clear();
    }

    pub fn prepare_public_baseline(&self) {
        self.discard_pending_save_candidates();
        *self.state.published_runtime.borrow_mut() = None;
    }

    pub fn step_frame(&mut self) -> Result<TickFrame, StepFatal> {
        self.step_frame_inner(false).map(|(frame, _)| frame)
    }

    /// step_frame 接轨件 4 的契约测试入口（cfg(test)）：显式走**旧管线**
    /// frame 路径——旧管线专有注入面/决策链契约（决策失败注入、epoch
    /// 对齐见证、NPC 日界准备 seam、旧簿 fixture）的测试锚定在旧语义
    /// 上，与 `GameSession::step_pipeline_contract` 同型登记（D2 先例：
    /// runtime 无对应注入面，经 step_frame 混跑只会把注入面缺失误报为
    /// 语义失败）。runtime 的等价锚由 flip 批/stepframe parity 承载。
    #[cfg(test)]
    pub fn step_frame_pipeline_contract(&mut self) -> Result<TickFrame, StepFatal> {
        let malformed = std::mem::take(&mut self.malformed_frame);
        let mut cursor = self.state.fact_cursor.clone();
        let ((frame, retained), _evidence) =
            crate::session::pipeline::execute_authoritative_tick_with_projection(
                &mut self.state.game,
                false,
                |projection, facts| {
                    let frame = projection.project_frame(facts)?;
                    let frame = malformed_frame_if_requested(frame, malformed);
                    frame.validate().map_err(protocol_fatal)?;
                    cursor.begin_tick(frame.tick);
                    cursor
                        .record_committed_facts(&frame.facts)
                        .map_err(protocol_fatal)?;
                    let retained = std::sync::Arc::new(frame.clone());
                    Ok((frame, retained))
                },
            )?;
        self.state.fact_cursor = cursor;
        self.state.intraday.push(retained);
        // committed_runtime 不写（tick_batch 是 publication 面——契约
        // 测试不消费）。
        Ok(frame)
    }

    /// 证据与 frame 来自同一 prepared candidate；提交前不外发任何事实。
    pub fn step_frame_with_commit_evidence(
        &mut self,
    ) -> Result<(TickFrame, crate::session::pipeline::TickCommitEvidence), StepFatal> {
        let (frame, evidence) = self.step_frame_inner(true)?;
        Ok((
            frame,
            evidence.expect("commit evidence was requested at the authoritative entry"),
        ))
    }

    fn step_frame_inner(
        &mut self,
        capture_commit_evidence: bool,
    ) -> Result<
        (
            TickFrame,
            Option<crate::session::pipeline::TickCommitEvidence>,
        ),
        StepFatal,
    > {
        #[cfg(test)]
        let malformed = std::mem::take(&mut self.malformed_frame);
        // 只复制数值游标；frame、runtime 和游标检查都在同版 candidate 的 precommit
        // callback 中完成。失败时从未安装 authority，不靠 install→rollback 修复。
        let mut cursor = self.state.fact_cursor.clone();
        let ((frame, retained, runtime), evidence) = self.state.game.step_frame_and_commit(
            capture_commit_evidence,
            |projection, facts| {
                let (frame, runtime) = projection.project_frame_and_runtime(facts)?;
                #[cfg(test)]
                let frame = malformed_frame_if_requested(frame, malformed);
                frame.validate().map_err(protocol_fatal)?;
                cursor.begin_tick(frame.tick);
                cursor
                    .record_committed_facts(&frame.facts)
                    .map_err(protocol_fatal)?;
                let retained = Arc::new(frame.clone());
                Ok((frame, retained, runtime))
            },
        )?;
        // 全部可失败工作已结束：只安装已验证记录与同版 runtime。
        self.state.fact_cursor = cursor;
        self.state.intraday.push(retained);
        self.state.committed_runtime = Some(runtime);
        Ok((frame, evidence))
    }

    pub fn end_civil_day_update(&mut self) -> Result<CivilUpdate, SessionError> {
        self.end_civil_day_update_classified()
            .map_err(super::CivilDayEndFailure::into_error)
    }

    /// 失败恢复资格来自本次真实 prepare；旧领域错误经 callback 返回不能继承该资格。
    pub fn end_civil_day_update_classified(
        &mut self,
    ) -> Result<CivilUpdate, super::CivilDayEndFailure> {
        self.state.game.require_healthy()?;
        #[cfg(test)]
        let malformed = std::mem::take(&mut self.malformed_civil);
        // 只有 civil publication 构建完整收盘数组；外层 checkpoint 仍共享既有历史。
        let intraday = self
            .state
            .intraday
            .iter()
            .map(|frame| frame.as_ref().clone())
            .collect::<Vec<_>>();
        let settled_phase = self.state.game.validate_civil_intraday(&intraday)?;
        let seq_from = self.state.game.seq();
        let mut cursor = self.state.fact_cursor.clone();
        cursor.begin_tick(self.state.game.tick());
        let (update, save) =
            self.state
                .game
                .with_prepared_civil_day_classified(|candidate, report| {
                    let update = candidate.project_civil_update(
                        intraday,
                        settled_phase,
                        seq_from,
                        report,
                        &cursor,
                    )?;
                    #[cfg(test)]
                    let update = {
                        let mut update = update;
                        if malformed {
                            update.seq_to += 1;
                        }
                        update
                    };
                    update.validate().map_err(protocol_fatal)?;
                    cursor
                        .record_committed_facts(&update.facts)
                        .map_err(protocol_fatal)?;
                    let save = Arc::new(candidate.save_committed_projection()?);
                    Ok((update, save))
                })?;
        // authority 已一次安装；此后仅移交同版已验证输出，不再有可失败投影。
        self.state.fact_cursor = cursor;
        self.state.intraday = AppendOnlyHistory::default();
        self.state.committed_runtime = None;
        self.state
            .pending_save_candidates
            .borrow_mut()
            .insert(save.snapshot.seq, Arc::clone(&save));
        self.state.day_end_save = Some(save);
        *self.state.published_runtime.borrow_mut() = None;
        Ok(update)
    }

    pub fn tick_batch(&self, frames: Vec<TickFrame>) -> Result<TickBatch, StepFatal> {
        let mut previous = self.state.published_runtime.borrow_mut();
        self.state.game.require_healthy()?;
        let current = self
            .state
            .committed_runtime
            .as_ref()
            .ok_or_else(|| protocol_fatal(super::super::ProtocolError::RuntimeDeltaMismatch))?;
        let batch = current.tick_batch_delta(frames, previous.as_ref())?;
        *previous = Some(current.clone());
        Ok(batch)
    }
}

// 显式转发现有领域命令；不授予权限，不替换 epoch，不触发推进或存档。
impl ProtocolSession {
    pub fn propose_player_company_action(
        &mut self,
        account: crate::AccountId,
        proposal: crate::session::PlayerCompanyProposal,
    ) -> Result<
        crate::session::PlayerCompanyCommandReceipt,
        crate::session::PlayerCompanyCommandError,
    > {
        self.state
            .game
            .propose_player_company_action(account, proposal)
    }

    pub fn enqueue_report_correction(
        &mut self,
        epoch: &crate::session::ReportCorrectionEpoch,
        request: crate::session::CompanyReportCorrection,
    ) -> Result<(), crate::session::ReportCorrectionError> {
        self.state.game.enqueue_report_correction(epoch, request)
    }

    pub fn cancel_report_correction(
        &mut self,
        epoch: &crate::session::ReportCorrectionEpoch,
        operation_id: &str,
    ) -> Result<(), crate::session::ReportCorrectionError> {
        self.state
            .game
            .cancel_report_correction(epoch, operation_id)
    }
}

#[cfg(test)]
fn malformed_frame_if_requested(mut frame: TickFrame, malformed: bool) -> TickFrame {
    if malformed {
        frame.seq_to = frame.seq_to.saturating_add(1);
    }
    frame
}

impl std::ops::Deref for ProtocolSession {
    type Target = GameSession;

    fn deref(&self) -> &Self::Target {
        &self.state.game
    }
}

fn protocol_fatal(error: super::ProtocolError) -> StepFatal {
    StepFatal::InvariantViolation {
        location: "ProtocolSession publication".to_owned(),
        description: error.to_string(),
    }
}

#[cfg(test)]
#[path = "day_end_npc_tests.rs"]
mod day_end_npc_tests;

#[cfg(test)]
#[path = "epoch_alignment_tests.rs"]
mod epoch_alignment_tests;

#[cfg(test)]
#[path = "decision_failure_witness_tests.rs"]
mod decision_failure_witness_tests;

#[cfg(test)]
#[path = "single_owner_tests.rs"]
mod single_owner_tests;

#[cfg(test)]
mod rollback_tests {
    use super::*;

    /// native callback 失败已经越过 World 提交；只核对未安装镜像，不公开绕过健康门的存档。
    #[cfg(not(target_family = "wasm"))]
    struct NativePublicationBaseline {
        save: crate::SaveSlot,
        business_hash: crate::session::StateHash,
        history: serde_json::Value,
        fact_cursor: crate::session::protocol::SessionFactCursor,
        committed_runtime: Option<serde_json::Value>,
        published_runtime: Option<serde_json::Value>,
        frozen_save: serde_json::Value,
        pending_saves: serde_json::Value,
    }

    #[cfg(not(target_family = "wasm"))]
    impl NativePublicationBaseline {
        fn capture(session: &ProtocolSession) -> Self {
            Self {
                save: session.state.game.save().unwrap(),
                business_hash: session.state.game.business_state_hash().unwrap(),
                history: serde_json::to_value(&session.state.intraday).unwrap(),
                fact_cursor: session.state.fact_cursor.clone(),
                committed_runtime: session
                    .state
                    .committed_runtime
                    .as_ref()
                    .map(|state| state.test_projection()),
                published_runtime: session
                    .state
                    .published_runtime
                    .borrow()
                    .as_ref()
                    .map(|state| state.test_projection()),
                frozen_save: serde_json::to_value(&session.state.day_end_save).unwrap(),
                pending_saves: serde_json::to_value(
                    &*session.state.pending_save_candidates.borrow(),
                )
                .unwrap(),
            }
        }

        fn assert_terminal(self, session: &mut ProtocolSession, fatal: StepFatal) {
            assert!(
                matches!(&fatal, StepFatal::InvariantViolation { location, description }
                    if location == "ProtocolSession publication"
                        && description.contains("event sequence coverage is not exactly")),
                "必须真实到达注入后的 publication 校验：{fatal:?}"
            );
            assert_eq!(session.state.game.poison_reason(), Some(&fatal));
            assert_eq!(
                session.state.game.business_state_hash().unwrap(),
                self.business_hash
            );
            assert_eq!(
                serde_json::to_value(
                    session
                        .state
                        .game
                        .save_projection(self.save.runtime_state.clone())
                )
                .unwrap(),
                serde_json::to_value(&self.save).unwrap(),
                "最后 publication 拒绝不得安装账户、T+1、公司、披露、日界或随机状态"
            );
            assert_eq!(
                serde_json::to_value(&session.state.intraday).unwrap(),
                self.history
            );
            assert_eq!(session.state.fact_cursor, self.fact_cursor);
            assert_eq!(
                session
                    .state
                    .committed_runtime
                    .as_ref()
                    .map(|state| state.test_projection()),
                self.committed_runtime
            );
            assert_eq!(
                session
                    .state
                    .published_runtime
                    .borrow()
                    .as_ref()
                    .map(|state| state.test_projection()),
                self.published_runtime
            );
            assert_eq!(
                serde_json::to_value(&session.state.day_end_save).unwrap(),
                self.frozen_save
            );
            assert_eq!(
                serde_json::to_value(&*session.state.pending_save_candidates.borrow()).unwrap(),
                self.pending_saves
            );
            assert!(
                matches!(session.save(), Err(SessionError::Step(ref actual)) if actual == &fatal)
            );
            assert!(
                matches!(session.game().save(), Err(SessionError::Step(ref actual)) if actual == &fatal)
            );
            assert!(matches!(session.checkpoint(), Err(ref actual) if actual == &fatal));
            assert_eq!(session.step_frame().unwrap_err(), fatal);
            assert!(
                matches!(session.end_civil_day_update(), Err(SessionError::Step(ref actual)) if actual == &fatal)
            );
            assert_eq!(
                session.state.fact_cursor, self.fact_cursor,
                "终态拒绝不能继续消耗事实身份"
            );
        }
    }

    #[test]
    fn publication_cursor_continues_at_same_tick_and_resets_on_next_tick() {
        use crate::session::protocol::SessionFactCursor;

        let event = |seq| crate::Event::CivilDateAdvanced {
            seq,
            settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        let mut cursor = SessionFactCursor::default();
        cursor.begin_tick(12);
        let first = cursor.assign_session_facts(vec![event(1)]).unwrap();
        assert_eq!(first[0].key.local_event_index(), 0);
        cursor.record_committed_facts(&first).unwrap();
        cursor.begin_tick(12);
        let second = cursor.assign_session_facts(vec![event(2)]).unwrap();
        assert_eq!(second[0].key.local_event_index(), 1);
        // 分配结果只有发布成功后才记入游标；游标本身不保留任何事件副本。
        cursor.record_committed_facts(&second).unwrap();
        let checkpoint = cursor.clone();
        cursor.begin_tick(13);
        assert_eq!(
            cursor.assign_session_facts(vec![event(3)]).unwrap()[0]
                .key
                .local_event_index(),
            0
        );
        cursor = checkpoint;
        assert_eq!(
            cursor.assign_session_facts(vec![event(4)]).unwrap()[0]
                .key
                .local_event_index(),
            2
        );
    }

    #[test]
    fn checkpoint_restores_published_runtime_after_a_successful_batch() {
        // 本例只验证 serial checkpoint 机械回放，证据入口不创建 runtime authority。
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = ProtocolSession::new(setup, 81).unwrap();
        let first = session
            .step_frame_with_commit_evidence()
            .map(|(frame, _)| frame)
            .unwrap();
        session.tick_batch(vec![first]).unwrap();
        let before = session
            .state
            .published_runtime
            .borrow()
            .as_ref()
            .unwrap()
            .test_projection();
        let checkpoint = session.checkpoint().unwrap();
        let frame = session
            .step_frame_with_commit_evidence()
            .map(|(frame, _)| frame)
            .unwrap();
        let first_batch =
            serde_json::to_value(session.tick_batch(vec![frame.clone()]).unwrap()).unwrap();
        assert_ne!(
            session
                .state
                .published_runtime
                .borrow()
                .as_ref()
                .unwrap()
                .test_projection(),
            before
        );
        session.rollback(checkpoint).unwrap();
        assert_eq!(
            session
                .state
                .published_runtime
                .borrow()
                .as_ref()
                .unwrap()
                .test_projection(),
            before
        );
        let retry = session
            .step_frame_with_commit_evidence()
            .map(|(frame, _)| frame)
            .unwrap();
        assert_eq!(
            serde_json::to_value(session.tick_batch(vec![retry]).unwrap()).unwrap(),
            first_batch
        );
    }

    #[test]
    fn checkpoint_restores_prior_candidates_and_frozen_save_after_two_civil_updates() {
        #[cfg(not(target_family = "wasm"))]
        if crate::runtime::step_runtime_enabled() {
            ProtocolSession::run_serial_checkpoint_probe_for_test();
            return;
        }
        assert!(
            !crate::runtime::step_runtime_enabled(),
            "civil 机械回放必须处于明确 serial 模式"
        );
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-04").unwrap();
        let steps = setup.ticks_per_day;
        let mut session = ProtocolSession::new(setup, 82).unwrap();
        for _ in 0..steps {
            session.step_frame().unwrap();
        }
        let old = session.end_civil_day_update().unwrap();
        let before = session.state.pending_save_candidates.borrow().clone();
        let frozen = session.state.day_end_save.clone().unwrap();
        let checkpoint = session.checkpoint().unwrap();
        session.end_civil_day_update().unwrap();
        session.end_civil_day_update().unwrap();
        assert_eq!(
            checkpoint.state.pending_save_candidates.borrow().len(),
            before.len()
        );
        assert_eq!(
            session.state.pending_save_candidates.borrow().len(),
            before.len() + 2
        );
        session.rollback(checkpoint).unwrap();
        assert_eq!(
            session
                .state
                .pending_save_candidates
                .borrow()
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            before.keys().copied().collect::<Vec<_>>()
        );
        assert!(Arc::ptr_eq(
            session.state.day_end_save.as_ref().unwrap(),
            &frozen
        ));
        let key = super::super::super::SaveCandidateKey {
            seq: old.seq_to,
            settled_date: old.boundary.settled_date,
        };
        let captured = session.save_candidate(&key).unwrap();
        assert_eq!(captured.snapshot.seq, frozen.snapshot.seq);
        assert_eq!(
            captured.civil_clock.settled_through,
            frozen.civil_clock.settled_through
        );
    }

    #[cfg(feature = "verification-harness")]
    #[test]
    fn verification_checkpoint_restores_authority_without_creating_a_public_save() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let session = ProtocolSession::new(setup, 77).unwrap();
        let checkpoint = session.game().save().unwrap();
        assert!(matches!(
            ProtocolSession::restore(&checkpoint),
            Err(SessionError::InvalidSave(_))
        ));
        let restored = ProtocolSession::restore_verification_checkpoint(&checkpoint).unwrap();
        assert_eq!(
            serde_json::to_vec(&restored.game().save().unwrap()).unwrap(),
            serde_json::to_vec(&checkpoint).unwrap()
        );
        assert!(matches!(restored.save(), Err(SessionError::InvalidSave(_))));
        assert!(matches!(
            restored.save_candidate(&crate::session::protocol::SaveCandidateKey {
                seq: checkpoint.snapshot.seq,
                settled_date: checkpoint.setup.start_date,
            }),
            Err(SessionError::InvalidSave(_))
        ));
    }

    #[test]
    fn day_end_restore_does_not_misclassify_the_next_unsettled_close_as_a_save() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let steps = setup.ticks_per_day;
        let mut session = ProtocolSession::new(setup, 78).unwrap();
        for _ in 0..steps {
            session.step_frame().unwrap();
        }
        session.end_civil_day_update().unwrap();
        for _ in 0..steps {
            session.step_frame().unwrap();
        }
        let raw_current_state = session.game().save().unwrap();
        assert!(matches!(
            ProtocolSession::restore(&raw_current_state),
            Err(SessionError::InvalidSave(_))
        ));
    }

    #[test]
    fn day_end_archive_excludes_late_ingress_without_losing_the_live_input() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        setup.npcs.retail_count = 0;
        setup.npcs.inst_count = 0;
        let steps = setup.ticks_per_day;
        let code = crate::StockId(0);
        let mut session = ProtocolSession::new(setup, 79).unwrap();
        for _ in 0..steps {
            session.step_frame().unwrap();
        }
        assert!(session.civil_day_ready().unwrap());
        session
            .shared_ingress()
            .enqueue_player_intent(
                crate::AccountId(0),
                crate::Intent::PlaceLimit {
                    code,
                    side: crate::Side::Buy,
                    price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
                    qty: 100,
                },
            )
            .unwrap();
        session.end_civil_day_update().unwrap();
        let archived = session.save().unwrap();
        assert!(archived.pending_player.is_empty());
        ProtocolSession::restore(&archived).unwrap();
        assert_eq!(session.game().save().unwrap().pending_player.len(), 1);
        let received = session.step_frame().unwrap();
        assert_eq!(received.events().filter(|event| matches!(event, crate::Event::OrderAccepted { account, code: received_code, .. } if *account == crate::AccountId(0) && *received_code == code)).count(), 1);
        let next = session.step_frame().unwrap();
        assert!(!next.events().any(|event| matches!(event, crate::Event::OrderAccepted { account, .. } if *account == crate::AccountId(0))));
    }

    #[cfg(feature = "verification-harness")]
    #[test]
    fn verification_fork_preserves_failed_concurrent_player_receipts_and_isolates_intake() {
        // 本例只验证 serial checkpoint 机械回放，证据入口不创建 runtime authority。
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        setup.npcs.retail_count = 0;
        setup.npcs.inst_count = 0;
        let code = crate::StockId(0);
        let intent = || crate::Intent::PlaceLimit {
            code,
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
            qty: 100,
        };
        let mut session = ProtocolSession::new(setup, 83).unwrap();
        let retained = session
            .step_frame_with_commit_evidence()
            .map(|(frame, _)| frame)
            .unwrap();
        let original = session.shared_ingress();
        let checkpoint = session.checkpoint().unwrap();
        let before = session.business_state_hash().unwrap();
        let player = intent();
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    original
                        .enqueue_player_intent(crate::AccountId(0), player)
                        .unwrap()
                })
                .join()
                .unwrap();
        });
        session.malformed_frame = true;
        assert!(session
            .step_frame_with_commit_evidence()
            .map(|(frame, _)| frame)
            .is_err());
        session.rollback(checkpoint).unwrap();
        assert_eq!(session.business_state_hash().unwrap(), before);
        let npc_intent = intent();
        let mut failed_private = session.state.game.clone_for_tick_shadow().unwrap();
        let discarded = failed_private
            .receive_private_intent(crate::AccountId(0), npc_intent)
            .unwrap();
        crate::session::pipeline::staged_decision::stage(
            crate::session::pipeline::NpcDecisionHandover::new(
                failed_private.tick(),
                Vec::new(),
                vec![discarded],
                Vec::new(),
            ),
        );
        drop(failed_private);
        // 影子丢弃语义（批四等价改接）：candidate 整体作废 ⇒ 批随之作废
        //（旧形态=影子内 pending 随 drop 消失；staged 槽同步清空）。
        crate::session::pipeline::staged_decision::clear();
        original.close().unwrap();
        let original_save = session.game().save().unwrap();
        assert_eq!(original_save.pending_player.len(), 1);
        assert_eq!(original_save.pending_player[0].account_ordinal, 0);
        assert_eq!(
            original_save.ingress_receipt_cursors.next_account_ordinal[&crate::AccountId(0)],
            2
        );
        let mut branch = session.fork_for_verification().unwrap();
        assert_eq!(
            serde_json::to_value(branch.game().save().unwrap()).unwrap(),
            serde_json::to_value(&original_save).unwrap()
        );
        branch
            .shared_ingress()
            .enqueue_player_intent(crate::AccountId(0), intent())
            .unwrap();
        let branch_save = branch.game().save().unwrap();
        assert_eq!(
            branch_save
                .pending_player
                .iter()
                .map(|receipt| receipt.account_ordinal)
                .collect::<Vec<_>>(),
            vec![0, 2]
        );
        assert_eq!(
            branch_save.ingress_receipt_cursors.next_account_ordinal[&crate::AccountId(0)],
            3
        );
        assert_eq!(
            serde_json::to_value(session.game().save().unwrap()).unwrap(),
            serde_json::to_value(original_save).unwrap()
        );
        assert!(original
            .enqueue_player_intent(crate::AccountId(0), intent())
            .is_err());
        let accepted = branch
            .step_frame_with_commit_evidence()
            .map(|(frame, _)| frame)
            .unwrap();
        assert_eq!(accepted.events().filter(|event| matches!(event, crate::Event::OrderAccepted { account, .. } if *account == crate::AccountId(0))).count(), 2);
        assert_eq!(
            serde_json::to_value(branch.state.intraday.iter().next().unwrap()).unwrap(),
            serde_json::to_value(retained).unwrap()
        );
        // 批四：跨 tick 载体删除——档不含 pending_npc 键（NPC 待处理事实随
        // 载体退役；本断言原验证批为空，键缺失为更强形态）。
        assert!(serde_json::to_value(branch.game().save().unwrap())
            .unwrap()
            .get("pending_npc")
            .is_none());
    }

    #[test]
    fn restore_rejects_public_saves_with_intraday_order_state() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 79).unwrap();
        session.end_civil_day_update().unwrap();
        let saved = session.save().unwrap();
        let mut candidate = saved.clone();
        candidate.resting_orders.insert(
            crate::StockId(9),
            vec![crate::Order {
                id: crate::OrderId(1),
                side: crate::Side::Buy,
                price: crate::Money::from_cents(1000),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: crate::Money::ZERO,
                owner: crate::AccountId(0),
                seq: 1,
            }],
        );
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));

        let mut candidate = serde_json::to_value(&saved).unwrap();
        candidate["auction_orders"] = serde_json::json!({ "9": [{
            "owner": 0, "side": "Buy", "limit": "1000", "qty": 100, "order_id": 1,
        }] });
        assert!(
            serde_json::from_value::<crate::SaveSlot>(candidate).is_err(),
            "统一存档严格拒绝旧 auction_orders 字段，不做格式转换"
        );

        let mut candidate = saved.clone();
        candidate
            .runtime_state
            .live_envelopes
            .push(crate::SavedLiveEnvelope {
                key: crate::SavedEnvelopeKey {
                    account: crate::AccountId(0),
                    stock: crate::StockId(9),
                    order: crate::OrderId(1),
                    side: crate::Side::Buy,
                },
                charged: crate::SavedFeeComponents {
                    commission: crate::Money::ZERO,
                    stamp_tax: crate::Money::ZERO,
                    transfer_fee: crate::Money::ZERO,
                },
                remaining_qty: 100,
                filled_qty: 0,
                filled_value: crate::Money::ZERO,
                limit: crate::Money::ZERO,
                purse: crate::Money::ZERO,
            });
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));

        let mut candidate = saved.clone();
        candidate
            .npc_order_lifecycles
            .push(crate::session::NpcOrderLifecycle {
                account: crate::AccountId(0),
                code: crate::StockId(9),
                order_id: crate::OrderId(1),
                placed_market_minute: 0,
                expires_market_minute: 1,
            });
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));

        let intent = crate::Intent::PlaceLimit {
            code: crate::StockId(9),
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(crate::Money::from_cents(1000)),
            qty: 100,
        };
        let mut candidate = saved.clone();
        candidate
            .pending_player
            .push(crate::session::ReceiptBearingIntent {
                owner: crate::AccountId(0),
                intent: intent.clone(),
                account_ordinal: 0,
                stock_ordinal: 0,
            });
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));

        // 批四终态：`pending_npc` 字段已从 SaveSlot 删除——本组 tamper 用例
        //（含 intents 的 Some 档/空占位档）统一改在**解码边界**拒绝：JSON
        // 携带 `pending_npc` 键（任意形态）⇒ fault2100（旧格式档不被接受；
        // 原「日终空占位合法」契约随 queue_empty_npc_at_day_end 退役）。
        let mut legacy = serde_json::to_value(&saved).unwrap();
        legacy["pending_npc"] = serde_json::json!({
            "observed_tick": saved.snapshot.tick,
            "observed_accounts": Vec::<u64>::new(),
            "intents": [{
                "owner": 0,
                "intent": { "PlaceLimit": { "code": 9, "side": "Buy", "price": "1000", "qty": 100 } },
                "account_ordinal": "0",
                "stock_ordinal": "0",
            }],
            "dependencies": [],
        });
        assert!(matches!(
            crate::decode_save_slot(
                &serde_json::to_vec(&legacy).unwrap(),
                &crate::session::persistence::SaveDecodeLimits::default()
            ),
            Err(SessionError::InvalidSave(fault)) if fault.code == 2100
        ));
        let mut empty_legacy = serde_json::to_value(&saved).unwrap();
        empty_legacy["pending_npc"] = serde_json::json!({
            "observed_tick": saved.snapshot.tick,
            "observed_accounts": [],
            "intents": [],
            "dependencies": [],
        });
        assert!(matches!(
            crate::decode_save_slot(
                &serde_json::to_vec(&empty_legacy).unwrap(),
                &crate::session::persistence::SaveDecodeLimits::default()
            ),
            Err(SessionError::InvalidSave(fault)) if fault.code == 2100
        ));
    }

    #[test]
    fn restore_rejects_completed_save_with_an_active_plan_execution_leg() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 80).unwrap();
        session.end_civil_day_update().unwrap();
        let mut candidate = session.save().unwrap();
        let code = crate::StockId(0);
        assert!(candidate.resting_orders.values().all(Vec::is_empty));
        let plan_id = candidate
            .plans
            .create(crate::plans::PlanOpen {
                account: crate::AccountId(2),
                code,
                direction: crate::Side::Buy,
                target: crate::plans::PlanTarget::ShareCount(200),
                opinion: crate::plans::PlanOpinion {
                    signal_score_bp: 3_000,
                    source: crate::plans::OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: crate::plans::Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: 0,
            })
            .unwrap();
        candidate
            .plans
            .install_execution(plan_id, 100, crate::Money::from_cents(1_000), 1_000)
            .unwrap();

        GameSession::restore(&candidate)
            .expect("the low-level snapshot API must retain restorable intraday plans");
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));
    }

    #[test]
    fn day_end_save_candidate_keeps_two_completed_days_until_capture() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-05").unwrap();
        let mut session = ProtocolSession::new(setup, 77).unwrap();
        let first = session.end_civil_day_update().unwrap();
        let second = session.end_civil_day_update().unwrap();
        let key = super::super::super::SaveCandidateKey {
            seq: first.seq_to,
            settled_date: first.boundary.settled_date,
        };
        let captured = session.save_candidate(&key).unwrap();
        assert_eq!(
            captured.snapshot.seq, first.seq_to,
            "an earlier day-end request must not capture a later day"
        );
        assert_ne!(first.seq_to, second.seq_to);
        let wrong_date = super::super::super::SaveCandidateKey {
            seq: second.seq_to,
            settled_date: first.boundary.settled_date,
        };
        assert!(session.save_candidate(&wrong_date).is_err());
        let second_key = super::super::super::SaveCandidateKey {
            seq: second.seq_to,
            settled_date: second.boundary.settled_date,
        };
        assert_eq!(
            session.save_candidate(&second_key).unwrap().snapshot.seq,
            second.seq_to
        );
        assert!(session.save_candidate(&key).is_err());
        session.discard_pending_save_candidates();
        assert_eq!(
            session.save_candidate(&second_key).unwrap().snapshot.seq,
            second.seq_to
        );
    }

    #[test]
    fn published_tick_batch_uses_player_delta_instead_of_full_snapshot() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = ProtocolSession::new(setup, 76).unwrap();
        let frame = session.step_frame().unwrap();
        let batch = session.tick_batch(vec![frame]).unwrap();
        assert!(batch.runtime_snapshot.is_none());
        let json = serde_json::to_value(batch).unwrap();
        assert!(json["runtime_delta"]["accounts"]
            .as_object()
            .unwrap()
            .contains_key("0"));
        let frame = session.step_frame().unwrap();
        let batch = serde_json::to_value(session.tick_batch(vec![frame]).unwrap()).unwrap();
        assert!(batch["runtime_delta"]["accounts"]
            .as_object()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn player_working_orders_reads_live_auction_without_a_save() {
        use crate::{Money, Side, StockId};
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 75).unwrap();
        session
            .state
            .game
            .fixture_append_auction_orders(
                StockId(0),
                vec![
                    crate::AuctionOrderSnap {
                        owner: crate::AccountId(0),
                        side: Side::Buy,
                        limit: Money::from_cents(1000),
                        qty: 100,
                        order_id: 81,
                    },
                    crate::AuctionOrderSnap {
                        owner: crate::AccountId(1),
                        side: Side::Sell,
                        limit: Money::from_cents(1000),
                        qty: 100,
                        order_id: 82,
                    },
                ],
            )
            .expect("追加竞价委托成功");
        assert!(session.save().is_err());
        let orders = session.player_working_orders();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].id, 81);
        assert_eq!(orders[0].remaining_qty, 100);
        assert_eq!(
            orders[0].venue,
            crate::session::protocol::player_orders::OrderVenue::Auction
        );
    }

    #[test]
    fn day_end_save_is_unavailable_before_first_completed_day() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let session = ProtocolSession::new(setup, 72).unwrap();
        assert!(session.save().is_err());
    }

    #[test]
    fn day_end_save_is_frozen_during_intraday_and_after_failed_day_end() {
        #[cfg(not(target_family = "wasm"))]
        if crate::runtime::step_runtime_enabled() {
            // 原完整回滚/重试合同仍由 serial exact 子进程执行；native 下继续核对终态零安装。
            ProtocolSession::run_serial_checkpoint_probe_for_test();
        }
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let steps = setup.ticks_per_day;
        let mut session = ProtocolSession::new(setup, 73).unwrap();
        for _ in 0..steps {
            session.step_frame().unwrap();
        }
        session.end_civil_day_update().unwrap();
        let saved = serde_json::to_value(session.save().unwrap()).unwrap();
        for _ in 0..steps {
            session.step_frame().unwrap();
        }
        assert_eq!(
            serde_json::to_value(session.save().unwrap()).unwrap(),
            saved
        );
        #[cfg(not(target_family = "wasm"))]
        let native_baseline = crate::runtime::step_runtime_enabled()
            .then(|| NativePublicationBaseline::capture(&session));
        session.malformed_civil = true;
        let error = session.end_civil_day_update().unwrap_err();
        assert!(matches!(error, SessionError::Step(_)));
        #[cfg(not(target_family = "wasm"))]
        if let Some(baseline) = native_baseline {
            let SessionError::Step(fatal) = error else {
                panic!("预期 publication 首错：{error:?}")
            };
            baseline.assert_terminal(&mut session, fatal);
            return;
        }
        assert_eq!(
            serde_json::to_value(session.save().unwrap()).unwrap(),
            saved
        );
        session.end_civil_day_update().unwrap();
        assert_ne!(
            serde_json::to_value(session.save().unwrap()).unwrap(),
            saved
        );
    }

    #[test]
    fn day_end_save_covers_closed_days_and_restores_candidate() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 74).unwrap();
        session.end_civil_day_update().unwrap();
        let saved = session.save().unwrap();
        let restored = ProtocolSession::restore(&saved).unwrap();
        assert_eq!(
            serde_json::to_value(restored.save().unwrap()).unwrap(),
            serde_json::to_value(saved).unwrap()
        );
    }

    fn active_retail_day_end_setup(start_date: &str) -> SessionSetup {
        let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
        setup.start_date = crate::CivilDate::from_iso(start_date).unwrap();
        setup.ticks_per_day = 12;
        setup.npcs.retail_count = 1;
        setup.npcs.inst_count = 0;
        setup.npcs.retail_cash_median = crate::Money::from_cents(10_000_000);
        setup.strategy_params.retail.arrival_rate = 1.0;
        setup.strategy_params.retail.chase_prob = 0.5;
        setup
    }

    fn assert_day_end_npc_continuation(mut live: ProtocolSession) {
        let tick = live.state.game.tick();
        let npc = crate::AccountId(1);
        let profile = live.state.game.state.accounts[&npc]
            .strategy()
            .unwrap()
            .profile();
        let analysis = crate::strategy::AnalysisProfile::new(
            crate::strategy::AnalysisWeights::new(0, 0, 10_000, 0, 0).unwrap(),
            None,
        )
        .unwrap();
        *live
            .state
            .game
            .state
            .belief_participants
            .get_mut(&npc)
            .unwrap()
            .belief_mut() = crate::strategy::BeliefBook::new(
            npc,
            profile,
            analysis,
            &mut crate::session::SplitMix64::new(99),
        );
        let attention = live
            .state
            .game
            .state
            .npc_attention
            .get_mut(&crate::AccountId(1))
            .unwrap();
        // 第一个 tick 接收真实玩家买单，下一 commit 的 NPC 观察可得到确定的买盘信号。
        attention.next_attention_candidate_tick = tick + 1;
        attention.rng_state = 3;
        let mut probe = attention.clone();
        assert!(probe.evaluate_candidate_with_signal(crate::AccountKind::Retail, 0.0, tick + 1));
        live.state.game.state.attention_scheduler.clear();
        live.state
            .game
            .state
            .attention_scheduler
            .enqueue(tick + 1, crate::AccountId(1));
        live.end_civil_day_update().unwrap();
        let slot = live.save().unwrap();
        assert!(
            serde_json::to_value(&slot)
                .unwrap()
                .get("pending_npc")
                .is_none(),
            "日终候选不能提前准备下一交易日的 NPC 批次（批四：键结构不存在）"
        );
        let mut restored = ProtocolSession::restore(&slot).unwrap();
        let frozen = serde_json::to_value(&slot).unwrap();
        let before_failed_tick = serde_json::to_value(restored.state.game.save().unwrap()).unwrap();
        restored.malformed_frame = true;
        assert!(restored.step_frame_pipeline_contract().is_err());
        assert_eq!(
            serde_json::to_value(restored.state.game.save().unwrap()).unwrap(),
            before_failed_tick,
            "首个 tick 失败时不能消耗 NPC 随机流或注意力状态"
        );
        assert_eq!(
            serde_json::to_value(restored.save().unwrap()).unwrap(),
            frozen
        );
        let code = crate::StockId(0);
        let buy = crate::Intent::PlaceLimit {
            code,
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(crate::Money::from_cents(900)),
            qty: 100,
        };
        live.enqueue_player_intent(crate::AccountId(0), buy.clone())
            .unwrap();
        restored
            .enqueue_player_intent(crate::AccountId(0), buy)
            .unwrap();
        let mut observed_npc_request = false;
        let mut diagnostics = Vec::new();
        for _ in 0..3 {
            let continued = live.step_frame_pipeline_contract().unwrap();
            let resumed = restored.step_frame_pipeline_contract().unwrap();
            diagnostics.push(format!(
                "events={:?}; decisions={:?}",
                continued.events().collect::<Vec<_>>(),
                live.state.game.state.last_retail_decisions,
            ));
            assert_eq!(
                continued.events().collect::<Vec<_>>(),
                resumed.events().collect::<Vec<_>>()
            );
            observed_npc_request |= continued.events().any(|event| {
                matches!(
                    event,
                    crate::Event::OrderAccepted {
                        account: crate::AccountId(1),
                        ..
                    } | crate::Event::IntentRejected {
                        account: crate::AccountId(1),
                        ..
                    }
                )
            });
            assert_eq!(
                serde_json::to_value(live.state.game.save().unwrap()).unwrap(),
                serde_json::to_value(restored.state.game.save().unwrap()).unwrap()
            );
        }
        assert!(
            observed_npc_request,
            "恢复后的首批 tick 必须实际执行 NPC 请求：{diagnostics:?}"
        );
        assert_eq!(serde_json::to_value(live.save().unwrap()).unwrap(), frozen);
    }

    #[test]
    fn due_day_boundary_npc_preparation_rolls_back_and_restores_exactly() {
        let npc = crate::AccountId(1);
        let mut setup = active_retail_day_end_setup("2030-01-01");
        // 只推进一个 tick；较细时间粒度保证真实 attention 下一次观察不会恰好落在首个 commit。
        setup.ticks_per_day = 1_000;
        let mut live = ProtocolSession::new(setup, 8).unwrap();
        let attention = live.state.game.state.npc_attention.get_mut(&npc).unwrap();
        attention.next_attention_candidate_tick = 0;
        attention.rng_state = 3;
        live.state.game.state.attention_scheduler.clear();
        live.state.game.state.attention_scheduler.enqueue(0, npc);
        live.end_civil_day_update().unwrap();
        let slot = live.save().unwrap();
        let mut restored = ProtocolSession::restore(&slot).unwrap();
        let before = serde_json::to_value(restored.state.game.save().unwrap()).unwrap();

        // 显式探针证明这个 fixture 会实际观察，不是 no-due 空批次捷径。
        let mut expected = live.state.game.clone_for_tick_shadow().unwrap();
        crate::session::pipeline::queue_npc_for_next_tick(&mut expected).unwrap();
        assert_eq!(
            crate::session::pipeline::staged_decision::take_any()
                .unwrap()
                .observed_accounts(),
            vec![npc]
        );
        assert_ne!(
            expected.state.npc_attention[&npc],
            live.state.game.state.npc_attention[&npc]
        );
        assert!(
            expected.state.npc_attention[&npc].next_attention_candidate_tick > 1,
            "fixture 的下一次观察必须晚于首个 commit，以隔离日界准备路径"
        );
        assert_eq!(
            serde_json::to_value(live.state.game.save().unwrap()).unwrap(),
            before,
            "准备探针不能改变权威 Session"
        );

        restored.malformed_frame = true;
        assert!(restored.step_frame_pipeline_contract().is_err());
        assert_eq!(
            serde_json::to_value(restored.state.game.save().unwrap()).unwrap(),
            before,
            "首 tick 后续失败必须回滚实际 NPC 观察及随机流"
        );
        let continued = live.step_frame_pipeline_contract().unwrap();
        let resumed = restored.step_frame_pipeline_contract().unwrap();
        assert_eq!(
            continued.events().collect::<Vec<_>>(),
            resumed.events().collect::<Vec<_>>()
        );
        assert_eq!(
            live.state.game.state.npc_attention[&npc], expected.state.npc_attention[&npc],
            "首 tick 必须在日界版本实际准备 NPC，而不是延后到首个 commit"
        );
        assert_eq!(
            serde_json::to_value(live.state.game.save().unwrap()).unwrap(),
            serde_json::to_value(restored.state.game.save().unwrap()).unwrap()
        );
        assert_eq!(
            serde_json::to_value(live.save().unwrap()).unwrap(),
            serde_json::to_value(slot).unwrap()
        );
    }

    #[test]
    fn closed_day_archive_defers_active_npc_requests_and_restores_exactly() {
        #[cfg(not(target_family = "wasm"))]
        if crate::runtime::step_runtime_enabled() {
            // fixture 使用旧管线 attention/ledger seam，整段保持同一 serial authority。
            ProtocolSession::run_serial_checkpoint_probe_for_test();
            return;
        }
        let live = ProtocolSession::new(active_retail_day_end_setup("2030-01-01"), 8).unwrap();
        assert_day_end_npc_continuation(live);
    }

    #[test]
    fn trading_day_archive_defers_active_npc_requests_and_restores_exactly() {
        #[cfg(not(target_family = "wasm"))]
        if crate::runtime::step_runtime_enabled() {
            // fixture 使用旧管线 attention/ledger seam，整段保持同一 serial authority。
            ProtocolSession::run_serial_checkpoint_probe_for_test();
            return;
        }
        let mut live = ProtocolSession::new(active_retail_day_end_setup("2030-01-02"), 8).unwrap();
        for _ in 0..11 {
            live.step_frame_pipeline_contract().unwrap();
        }
        let attention = live
            .state
            .game
            .state
            .npc_attention
            .get_mut(&crate::AccountId(1))
            .unwrap();
        attention.next_attention_candidate_tick = 12;
        attention.rng_state = 3;
        live.state.game.state.attention_scheduler.clear();
        live.state
            .game
            .state
            .attention_scheduler
            .enqueue(12, crate::AccountId(1));
        live.step_frame_pipeline_contract().unwrap();
        assert_day_end_npc_continuation(live);
    }

    #[test]
    fn checkpoint_shares_completed_intraday_frames_until_new_work() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        setup.ticks_per_day = 48;
        let mut session = ProtocolSession::new(setup, 50).unwrap();
        for _ in 0..33 {
            session.step_frame().unwrap();
        }
        let checkpoint = session.checkpoint().unwrap();
        let history = serde_json::to_value(&checkpoint.state.intraday).unwrap();

        assert!(
            std::ptr::eq(&session.state.intraday[0], &checkpoint.state.intraday[0]),
            "creating a rollback point must share completed frames, not copy the trading day"
        );
        session.step_frame().unwrap();
        assert!(
            std::ptr::eq(&session.state.intraday[0], &checkpoint.state.intraday[0]),
            "appending a frame must not copy older completed history chunks"
        );
        assert_eq!(
            serde_json::to_value(&checkpoint.state.intraday).unwrap(),
            history
        );
        assert_eq!(
            session.state.intraday.len(),
            checkpoint.state.intraday.len() + 1
        );
    }

    #[test]
    fn checkpoint_append_shares_frames_in_the_incomplete_tail_chunk() {
        // 本例只验证 serial checkpoint 机械回放，证据入口不创建 runtime authority。
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        setup.ticks_per_day = 48;
        let mut session = ProtocolSession::new(setup, 50).unwrap();
        for _ in 0..33 {
            session
                .step_frame_with_commit_evidence()
                .map(|(frame, _)| frame)
                .unwrap();
        }
        let checkpoint = session.checkpoint().unwrap();
        let before = serde_json::to_value(&checkpoint.state.intraday).unwrap();
        session
            .step_frame_with_commit_evidence()
            .map(|(frame, _)| frame)
            .unwrap();
        assert!(
            std::ptr::eq(
                &session.state.intraday[32].timeseries_payload,
                &checkpoint.state.intraday[32].timeseries_payload,
            ),
            "宿主批次回滚点不得让未满 chunk 中的已提交 TickFrame 深复制"
        );
        session.rollback(checkpoint).unwrap();
        assert_eq!(
            serde_json::to_value(&session.state.intraday).unwrap(),
            before
        );
        assert_eq!(session.tick(), 33);
        assert_eq!(
            session
                .step_frame_with_commit_evidence()
                .map(|(frame, _)| frame)
                .unwrap()
                .tick,
            34
        );
    }

    #[test]
    fn outer_rollback_restores_runtime_diagnostics_without_reloading_a_save() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 51).unwrap();
        session.state.game.state.last_retail_order_events.push(
            crate::session::RetailOrderDiagnosticEvent::Rejected {
                account: crate::AccountId(1),
                code: crate::StockId(0),
                reason: crate::RejectionReason::InsufficientCash,
            },
        );
        let before = session.state.game.session_state_hash().unwrap();
        let diagnostics =
            serde_json::to_value(session.state.game.last_retail_order_events()).unwrap();
        let checkpoint = session.checkpoint().unwrap();
        session.state.game.state.last_retail_order_events.clear();

        session.rollback(checkpoint).unwrap();

        assert_eq!(session.state.game.session_state_hash().unwrap(), before);
        assert_eq!(
            serde_json::to_value(session.state.game.last_retail_order_events()).unwrap(),
            diagnostics
        );
    }

    #[test]
    fn outer_rollback_across_close_and_weekend_restores_history_and_can_repeat() {
        #[cfg(not(target_family = "wasm"))]
        if crate::runtime::step_runtime_enabled() {
            ProtocolSession::run_serial_checkpoint_probe_for_test();
            return;
        }
        assert!(
            !crate::runtime::step_runtime_enabled(),
            "civil 机械回放必须处于明确 serial 模式"
        );
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-04").unwrap();
        let steps_before_close = setup.ticks_per_day - 1;
        let mut session = ProtocolSession::new(setup, 52).unwrap();
        for _ in 0..steps_before_close {
            session.step_frame().unwrap();
        }
        let before = session.state.game.session_state_hash().unwrap();
        let history = serde_json::to_value(&session.state.intraday).unwrap();
        let fact_tick = session.state.fact_cursor.tick();
        let cursor_before = session.state.fact_cursor.clone();
        let checkpoint = session.checkpoint().unwrap();
        let frame = session.step_frame().unwrap();
        let closing = session.end_civil_day_update().unwrap();
        let weekend = session.end_civil_day_update().unwrap();
        assert!(session.state.intraday.is_empty());
        assert_eq!(weekend.civil_date, "2030-01-06");

        session.rollback(checkpoint).unwrap();

        assert_eq!(session.state.game.session_state_hash().unwrap(), before);
        assert_eq!(
            serde_json::to_value(&session.state.intraday).unwrap(),
            history
        );
        assert_eq!(session.state.fact_cursor.tick(), fact_tick);
        assert_eq!(session.state.fact_cursor, cursor_before);
        assert_eq!(
            serde_json::to_value(session.step_frame().unwrap()).unwrap(),
            serde_json::to_value(frame).unwrap()
        );
        assert_eq!(
            serde_json::to_value(session.end_civil_day_update().unwrap()).unwrap(),
            serde_json::to_value(closing).unwrap()
        );
        assert_eq!(
            serde_json::to_value(session.end_civil_day_update().unwrap()).unwrap(),
            serde_json::to_value(weekend).unwrap()
        );
    }

    #[test]
    fn checkpoint_rejects_poison_and_failed_candidate_can_retry_from_healthy_state() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        // 本用例验证市场候选提交及回滚，首拍须为开市日。
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = ProtocolSession::new(setup, 53).unwrap();
        let fatal = StepFatal::InvariantViolation {
            location: "protocol checkpoint test".into(),
            description: "injected candidate failure".into(),
        };
        let before = session.state.game.business_state_hash().unwrap();
        session.state.game.inject_step_failure(fatal.clone());
        assert_eq!(session.step_frame().unwrap_err(), fatal);
        assert_eq!(session.state.game.business_state_hash().unwrap(), before);
        assert_eq!(session.state.game.poison_reason(), None);
        session.step_frame().unwrap().validate().unwrap();

        session.state.game.poison_failed_step(fatal.clone());
        assert!(matches!(session.checkpoint(), Err(error) if error == fatal));
    }

    #[test]
    fn committed_evidence_frame_is_the_same_public_runtime_step() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        // 本用例验证市场候选提交及回滚，首拍须为开市日。
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut ordinary = ProtocolSession::new(setup.clone(), 47).unwrap();
        let mut observed = ProtocolSession::new(setup, 47).unwrap();

        let expected = ordinary.step_frame().unwrap();
        let (actual, evidence) = observed.step_frame_with_commit_evidence().unwrap();

        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(
            evidence.next_receipt_index(),
            observed
                .game()
                .save()
                .unwrap()
                .runtime_state
                .next_receipt_base
        );
    }

    #[test]
    fn malformed_evidence_frame_rolls_back_game_history_and_facts_then_retries() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        // 本用例验证市场候选提交及回滚，首拍须为开市日。
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = ProtocolSession::new(setup, 48).unwrap();
        let preceding = crate::Event::CivilDateAdvanced {
            seq: session.state.game.seq(),
            settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        session
            .state
            .fact_cursor
            .begin_tick(session.state.game.tick());
        let preceding_facts = session
            .state
            .fact_cursor
            .assign_session_facts(vec![preceding])
            .unwrap();
        session
            .state
            .fact_cursor
            .record_committed_facts(&preceding_facts)
            .unwrap();
        let before = serde_json::to_value(session.state.game.save().unwrap()).unwrap();
        let history = serde_json::to_value(&session.state.intraday).unwrap();
        let fact_tick = session.state.fact_cursor.tick();
        let cursor_before = session.state.fact_cursor.clone();
        session.malformed_frame = true;

        let error = session.step_frame_with_commit_evidence().unwrap_err();

        assert!(matches!(error, StepFatal::InvariantViolation { .. }));
        assert_eq!(
            serde_json::to_value(session.state.game.save().unwrap()).unwrap(),
            before
        );
        assert_eq!(
            serde_json::to_value(&session.state.intraday).unwrap(),
            history
        );
        assert_eq!(session.state.fact_cursor.tick(), fact_tick);
        assert_eq!(session.state.fact_cursor, cursor_before);
        let (frame, evidence) = session.step_frame_with_commit_evidence().unwrap();
        frame.validate().unwrap();
        assert_eq!(
            evidence.next_receipt_index(),
            session
                .game()
                .save()
                .unwrap()
                .runtime_state
                .next_receipt_base
        );
    }

    #[test]
    fn malformed_civil_after_mutation_rolls_back_and_can_retry() {
        #[cfg(not(target_family = "wasm"))]
        if crate::runtime::step_runtime_enabled() {
            // 原完整回滚/重试合同仍由 serial exact 子进程执行；native 下继续核对终态零安装。
            ProtocolSession::run_serial_checkpoint_probe_for_test();
        }
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = ProtocolSession::new(setup, 41).unwrap();
        while !session.civil_day_ready().unwrap() {
            session.step_frame().unwrap();
        }
        let before = serde_json::to_value(session.state.game.save().unwrap()).unwrap();
        let history = serde_json::to_value(&session.state.intraday).unwrap();
        #[cfg(not(target_family = "wasm"))]
        let native_baseline = crate::runtime::step_runtime_enabled()
            .then(|| NativePublicationBaseline::capture(&session));
        session.malformed_civil = true;

        let error = session.end_civil_day_update().unwrap_err();

        assert!(matches!(error, SessionError::Step(_)));
        #[cfg(not(target_family = "wasm"))]
        if let Some(baseline) = native_baseline {
            let SessionError::Step(fatal) = error else {
                panic!("预期 publication 首错：{error:?}")
            };
            baseline.assert_terminal(&mut session, fatal);
            return;
        }
        assert_eq!(
            serde_json::to_value(session.state.game.save().unwrap()).unwrap(),
            before
        );
        assert_eq!(
            serde_json::to_value(&session.state.intraday).unwrap(),
            history
        );
        let update = session.end_civil_day_update().unwrap();
        update.validate().unwrap();
        assert_eq!(
            serde_json::to_value(&update.refresh.intraday).unwrap(),
            history
        );
    }

    /// Sol 终审修复 1a：日结拍的投影失败不得泄漏 `day`——
    /// install_runtime_day_boundary 已推进 day（day_end.rs），回滚基线
    /// 此前不含 day 域（tick/seq/账户回滚而 day 留新日——违反失败零
    /// 泄漏及重试契约）。红态：失败后 day==day_before+1。
    #[test]
    fn solfinal1a_day_end_projection_failure_rolls_back_day() {
        #[cfg(not(target_family = "wasm"))]
        if crate::runtime::step_runtime_enabled() {
            // 原完整回滚/重试合同仍由 serial exact 子进程执行；native 下继续核对终态零安装。
            ProtocolSession::run_serial_checkpoint_probe_for_test();
        }
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        // 本用例验证市场候选提交及回滚，首拍须为开市日。
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = ProtocolSession::new(setup, 49).unwrap();
        let tpd = session.game().state.setup.ticks_per_day;
        for _ in 1..tpd {
            session.step_frame().unwrap();
        }
        let day_before = session.game().day();
        let before = serde_json::to_value(session.state.game.save().unwrap()).unwrap();
        #[cfg(not(target_family = "wasm"))]
        let native_baseline = crate::runtime::step_runtime_enabled()
            .then(|| NativePublicationBaseline::capture(&session));
        session.malformed_frame = true;
        let error = session.step_frame().unwrap_err();
        assert!(matches!(error, StepFatal::InvariantViolation { .. }));
        #[cfg(not(target_family = "wasm"))]
        if let Some(baseline) = native_baseline {
            baseline.assert_terminal(&mut session, error);
            return;
        }
        assert_eq!(
            session.game().day(),
            day_before,
            "日结拍投影失败必须回滚 day（不得泄漏新日——重试契约要求步前等价）"
        );
        assert_eq!(
            serde_json::to_value(session.state.game.save().unwrap()).unwrap(),
            before,
            "日结拍投影失败后 save==步前（含 day 域）"
        );
        // 重试成功：day 恰推进一次（candidate 丢弃可重试语义）。
        let frame = session.step_frame().unwrap();
        frame.validate().unwrap();
        assert_eq!(
            session.game().day(),
            day_before + 1,
            "重试成功推进 day 恰一次"
        );
    }

    #[test]
    fn malformed_frame_after_mutation_rolls_back_game_history_and_fact_cursor() {
        #[cfg(not(target_family = "wasm"))]
        if crate::runtime::step_runtime_enabled() {
            // 原完整回滚/重试合同仍由 serial exact 子进程执行；native 下继续核对终态零安装。
            ProtocolSession::run_serial_checkpoint_probe_for_test();
        }
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        // 本用例验证市场候选提交及回滚，首拍须为开市日。
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = ProtocolSession::new(setup, 40).unwrap();
        #[cfg(not(target_family = "wasm"))]
        if crate::runtime::step_runtime_enabled() {
            session
                .state
                .game
                .construct_runtime_driver_lazy_for_test()
                .unwrap();
        }
        let preceding = crate::Event::CivilDateAdvanced {
            seq: session.state.game.seq(),
            settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        session
            .state
            .fact_cursor
            .begin_tick(session.state.game.tick());
        let preceding_facts = session
            .state
            .fact_cursor
            .assign_session_facts(vec![preceding])
            .unwrap();
        session
            .state
            .fact_cursor
            .record_committed_facts(&preceding_facts)
            .unwrap();
        let before = serde_json::to_value(session.state.game.save().unwrap()).unwrap();
        let history = serde_json::to_value(&session.state.intraday).unwrap();
        let fact_tick = session.state.fact_cursor.tick();
        let cursor_before = session.state.fact_cursor.clone();
        #[cfg(not(target_family = "wasm"))]
        let native_baseline = crate::runtime::step_runtime_enabled()
            .then(|| NativePublicationBaseline::capture(&session));
        session.malformed_frame = true;

        let error = session.step_frame().unwrap_err();

        assert!(matches!(error, StepFatal::InvariantViolation { .. }));
        #[cfg(not(target_family = "wasm"))]
        if let Some(baseline) = native_baseline {
            baseline.assert_terminal(&mut session, error);
            return;
        }
        assert_eq!(
            serde_json::to_value(session.state.game.save().unwrap()).unwrap(),
            before
        );
        assert_eq!(
            serde_json::to_value(&session.state.intraday).unwrap(),
            history
        );
        assert_eq!(session.state.fact_cursor.tick(), fact_tick);
        assert_eq!(session.state.fact_cursor, cursor_before);
        session.step_frame().unwrap().validate().unwrap();
    }

    #[test]
    fn adjacent_civil_updates_continue_but_restore_starts_a_fresh_publication_stream() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-04").unwrap();
        let mut uninterrupted = ProtocolSession::new(setup, 43).unwrap();
        while !uninterrupted.civil_day_ready().unwrap() {
            uninterrupted.step_frame().unwrap();
        }
        let first = uninterrupted.end_civil_day_update().unwrap();
        assert_eq!(first.civil_date, "2030-01-05");
        let saved = uninterrupted.state.game.save().unwrap();
        let mut restored = ProtocolSession::restore(&saved).unwrap();

        let continued = uninterrupted.end_civil_day_update().unwrap();
        let fresh = restored.end_civil_day_update().unwrap();

        assert_eq!(
            continued.events().collect::<Vec<_>>(),
            fresh.events().collect::<Vec<_>>()
        );
        let continued_indices = continued
            .facts
            .iter()
            .filter(|fact| is_session_lifecycle_fact(fact))
            .map(|fact| fact.key.local_event_index())
            .collect::<Vec<_>>();
        let fresh_indices = fresh
            .facts
            .iter()
            .filter(|fact| is_session_lifecycle_fact(fact))
            .map(|fact| fact.key.local_event_index())
            .collect::<Vec<_>>();
        assert!(!continued_indices.is_empty());
        assert_eq!(fresh_indices.first(), Some(&0));
        let preceding_count = first
            .facts
            .iter()
            .filter(|fact| is_session_lifecycle_fact(fact))
            .count() as u64;
        assert_eq!(continued_indices.first(), Some(&preceding_count));
    }

    fn is_session_lifecycle_fact(fact: &crate::session::protocol::EventFact) -> bool {
        fact.key.phase_rank() == 6
            && fact.key.entity() == &crate::session::pipeline::EntityTag::Session
            && fact.key.source() == crate::session::pipeline::EventSourceIndex::Session
    }
}

#[cfg(all(test, feature = "simulation-diagnostics"))]
mod d0b_storage_tests {
    use super::*;
    use crate::diagnostics::{causal::CausalFactKind, NpcDecisionTraceRecord};
    use crate::{AccountId, OrderId, PlanId, StockId};

    fn session(seed: u64) -> ProtocolSession {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        setup.npcs.retail_count = 0;
        setup.npcs.inst_count = 0;
        setup.ticks_per_day = 30;
        ProtocolSession::new(setup, seed).unwrap()
    }

    fn trace(tick: u64) -> NpcDecisionTraceRecord {
        NpcDecisionTraceRecord {
            account: AccountId(17),
            tick,
            source_report_ids: vec![format!("report-{tick}")],
            expectation_method: Some("真实观察".into()),
            plan_ids: vec![PlanId {
                owner: AccountId(17),
                sequence: 1,
            }],
            plan_changes: vec!["初始变化".into()],
            budget_constraints: vec!["初始约束".into()],
            order_ids: vec![OrderId(1)],
            codes: vec![StockId(0)],
        }
    }

    fn seed(session: &mut ProtocolSession) {
        let game = &mut session.state.game;
        let time = game.causal_time().unwrap();
        // 纯 storage fixture，固定本股值，不冒 Calendar producer 或有效成交报告。
        let stock_time = crate::diagnostics::causal::FactTime {
            stock_clock: Some(crate::diagnostics::causal::StockFactClock {
                code: StockId(0),
                completed_minutes: 0,
            }),
            ..time
        };
        for key in 0..257u64 {
            let account = AccountId(key << 32);
            game.state.causal.record_plan_root(
                time,
                account,
                [CausalFactKind::Decision { account }],
            );
            game.state.causal.record_continuous_fill(
                stock_time,
                OrderId((1 << 40) + key),
                account,
                &StockId(0),
                100,
                key as i64 - 128,
            );
        }
        for tick in 0..128 {
            game.state.npc_decision_traces.record(trace(tick));
        }
        game.state.npc_decision_traces.enrich(
            AccountId(17),
            127,
            [OrderId(2), OrderId(1), OrderId(2)],
            [
                PlanId {
                    owner: AccountId(17),
                    sequence: 2,
                },
                PlanId {
                    owner: AccountId(17),
                    sequence: 1,
                },
            ],
            ["变化一".into(), "变化二".into()],
            ["约束一".into(), "约束二".into()],
        );
    }

    fn advance_seed(session: &mut ProtocolSession, tick: u64) {
        let game = &mut session.state.game;
        let time = game.causal_time().unwrap();
        // 纯 storage fixture，固定本股值，不冒 Calendar producer 或有效成交报告。
        let stock_time = crate::diagnostics::causal::FactTime {
            stock_clock: Some(crate::diagnostics::causal::StockFactClock {
                code: StockId(0),
                completed_minutes: 0,
            }),
            ..time
        };
        game.state.causal.record_plan_root(
            time,
            AccountId(17 << 32),
            [CausalFactKind::Decision {
                account: AccountId(17 << 32),
            }],
        );
        game.state.causal.record_continuous_fill(
            stock_time,
            OrderId((1 << 40) + 17),
            AccountId(17 << 32),
            &StockId(0),
            100,
            tick as i64,
        );
        game.state.npc_decision_traces.record(trace(tick));
        game.state.npc_decision_traces.enrich(
            AccountId(17),
            tick,
            [OrderId(3), OrderId(3)],
            [
                PlanId {
                    owner: AccountId(17),
                    sequence: 3,
                },
                PlanId {
                    owner: AccountId(17),
                    sequence: 3,
                },
            ],
            [format!("变化-{tick}")],
            [format!("约束-{tick}")],
        );
    }

    #[derive(Debug, PartialEq)]
    struct Snapshot {
        causal: Vec<u8>,
        trace: Vec<u8>,
        facts: Vec<u8>,
        query: Vec<NpcDecisionTraceRecord>,
        hash: crate::session::StateHash,
    }

    fn snapshot(session: &ProtocolSession) -> Snapshot {
        let game = session.game();
        let before_save = serde_json::to_vec(&game.save().unwrap()).unwrap();
        let before_hash = game.session_state_hash().unwrap();
        let query = game.npc_decision_trace(AccountId(17)).unwrap();
        assert_eq!(query, game.npc_decision_trace(AccountId(17)).unwrap());
        assert!(game.npc_decision_trace(AccountId(18)).is_none());
        let facts = game.causal_facts();
        for (index, fact) in facts.iter().enumerate() {
            assert_eq!(fact.sequence, index as u64);
        }
        let result = Snapshot {
            causal: serde_json::to_vec(&game.state.causal).unwrap(),
            trace: serde_json::to_vec(&game.state.npc_decision_traces).unwrap(),
            facts: serde_json::to_vec(facts).unwrap(),
            query,
            hash: before_hash,
        };
        assert_eq!(game.session_state_hash().unwrap(), before_hash);
        assert_eq!(
            serde_json::to_vec(&game.save().unwrap()).unwrap(),
            before_save
        );
        result
    }

    fn place(session: &mut ProtocolSession, cents: i64) -> OrderId {
        session
            .enqueue_player_intent(
                AccountId(0),
                crate::Intent::PlaceLimit {
                    code: StockId(0),
                    side: crate::Side::Buy,
                    price: crate::LimitPrice::Fixed(crate::Money::from_cents(cents)),
                    qty: 100,
                },
            )
            .unwrap();
        session
            .step_frame_with_commit_evidence()
            .map(|(frame, _)| frame)
            .unwrap()
            .events()
            .find_map(|event| match event {
                crate::Event::OrderAccepted { id, .. } => Some(*id),
                _ => None,
            })
            .expect("真实 player 申报必须成功")
    }

    fn cancel(session: &mut ProtocolSession, order: OrderId) {
        session
            .enqueue_player_intent(
                AccountId(0),
                crate::Intent::Cancel {
                    code: StockId(0),
                    id: order,
                },
            )
            .unwrap();
        assert!(session
            .step_frame_with_commit_evidence()
            .map(|(frame, _)| frame)
            .unwrap()
            .events()
            .any(|event| matches!(event, crate::Event::OrderCanceled { id, .. } if *id == order)));
    }

    #[test]
    fn d0b_protocol_nonempty_owned_checkpoints_restore_eviction_non_lifo_and_cross_session() {
        let mut live = session(7);
        seed(&mut live);
        let initial = snapshot(&live);
        assert_eq!(live.game().causal_facts().len(), 514);
        let oldest = live.checkpoint().unwrap();
        let cross_session = live.checkpoint().unwrap();
        let order = place(&mut live, 995);
        advance_seed(&mut live, 128);
        let middle = snapshot(&live);
        let newer = live.checkpoint().unwrap();
        cancel(&mut live, order);
        advance_seed(&mut live, 129);
        assert_eq!(
            live.game()
                .npc_decision_trace(AccountId(17))
                .unwrap()
                .first()
                .unwrap()
                .tick,
            2
        );
        assert_ne!(snapshot(&live), middle);
        live.rollback(oldest).unwrap();
        assert_eq!(snapshot(&live), initial);
        let restored = live.game().npc_decision_trace(AccountId(17)).unwrap();
        assert_eq!(restored.len(), 128);
        assert_eq!(restored.first().unwrap().tick, 0);
        assert_eq!(restored.last().unwrap().tick, 127);
        assert_eq!(restored.last().unwrap().order_ids, [OrderId(1), OrderId(2)]);
        assert_eq!(
            restored.last().unwrap().plan_ids,
            [
                PlanId {
                    owner: AccountId(17),
                    sequence: 1
                },
                PlanId {
                    owner: AccountId(17),
                    sequence: 2
                }
            ]
        );
        assert_eq!(
            restored.last().unwrap().plan_changes,
            ["初始变化", "变化一", "变化二"]
        );
        assert_eq!(
            restored.last().unwrap().budget_constraints,
            ["初始约束", "约束一", "约束二"]
        );
        live.rollback(newer).unwrap();
        assert_eq!(snapshot(&live), middle);
        let mut other = session(8);
        seed(&mut other);
        place(&mut other, 990);
        advance_seed(&mut other, 130);
        other.rollback(cross_session).unwrap();
        assert_eq!(snapshot(&other), initial);
        let causal: serde_json::Value = serde_json::from_slice(&initial.causal).unwrap();
        for key in 0..257u64 {
            assert_eq!(causal["decision"][format!("{}", key << 32)], key * 2);
            assert_eq!(
                causal["filled_values"][format!("{}", (1u64 << 40) + key)],
                (key as i64 - 128).to_string()
            );
        }
    }

    #[cfg(feature = "verification-harness")]
    #[test]
    fn d0b_protocol_verification_forks_keep_nonempty_diagnostics_independent() {
        let mut source = session(9);
        seed(&mut source);
        let initial = snapshot(&source);
        let mut left = source.fork_for_verification().unwrap();
        let mut right = source.fork_for_verification().unwrap();
        assert_eq!(snapshot(&left), initial);
        assert_eq!(snapshot(&right), initial);
        let left_order = place(&mut left, 995);
        advance_seed(&mut left, 128);
        let left_snapshot = snapshot(&left);
        assert_eq!(snapshot(&right), initial);
        assert_eq!(snapshot(&source), initial);
        let right_order = place(&mut right, 990);
        advance_seed(&mut right, 129);
        let right_snapshot = snapshot(&right);
        assert_ne!(left_snapshot, right_snapshot);
        assert_eq!(snapshot(&left), left_snapshot);
        let left_checkpoint = left.checkpoint().unwrap();
        let right_checkpoint = right.checkpoint().unwrap();
        cancel(&mut left, left_order);
        advance_seed(&mut left, 130);
        cancel(&mut right, right_order);
        advance_seed(&mut right, 131);
        left.rollback(left_checkpoint).unwrap();
        right.rollback(right_checkpoint).unwrap();
        assert_eq!(snapshot(&left), left_snapshot);
        assert_eq!(snapshot(&right), right_snapshot);
        assert_eq!(snapshot(&source), initial);
    }
}
