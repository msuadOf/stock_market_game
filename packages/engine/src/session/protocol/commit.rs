use super::{
    AuctionPoint, AuctionPointKind, ContinuousPoint, EventFact, PublicRuntimeState, TickFrame,
    TickTimeseriesPayload,
};
use crate::session::{Event, GameSession, StepFatal};

/// 借用已准备 tick candidate 的只读结构；禁止深克隆 GameSession。
/// frame/runtime 投影都来自同一版冻结 candidate，不重读 live authority（#4 冻结门禁）。
pub(in crate::session) struct TickCandidateProjection<'candidate> {
    candidate: &'candidate GameSession,
    seq_from: u64,
}

impl<'candidate> TickCandidateProjection<'candidate> {
    pub(crate) fn new(candidate: &'candidate GameSession, seq_from: u64) -> Self {
        Self {
            candidate,
            seq_from,
        }
    }

    pub fn tick(&self) -> u64 {
        self.candidate.tick()
    }

    pub fn seq_to(&self) -> u64 {
        self.candidate.seq()
    }

    pub fn project_frame(&self, facts: Vec<EventFact>) -> Result<TickFrame, StepFatal> {
        self.project_frame_and_runtime(facts)
            .map(|(frame, _)| frame)
    }

    pub(in crate::session) fn project_frame_and_runtime(
        &self,
        facts: Vec<EventFact>,
    ) -> Result<(TickFrame, PublicRuntimeState), StepFatal> {
        let snapshot = self.candidate.runtime_snapshot();
        let runtime = self.candidate.public_runtime_from_snapshot(&snapshot)?;
        let timeseries_payload = project_timeseries(&facts, snapshot);
        Ok((
            TickFrame {
                facts,
                tick: self.tick(),
                timeseries_payload,
                seq_from: self.seq_from,
                seq_to: self.seq_to(),
            },
            runtime,
        ))
    }
}

impl GameSession {
    /// 宿主市场 tick 入口（ADR-0042 两阶段模型；完整链=docs/architecture.md
    /// 「Escrow tick 与验证边界」节）：**D 决策阶段**（普通 NPC 按账户并行
    /// +计划 root capture 闭环，见 `plan_tick`/`apply_session_market_transaction`）
    /// →**E 执行阶段**（股票并行撮合/收尾+continuation 续行）→**publication
    /// 一次汇合**（收据聚合/结算/公开 view 构造验证）→**commit 单点安装**。
    /// 本方法只做 publication 回调包装与 poison 处理，不做阶段本体。
    pub fn step_frame(&mut self) -> Result<TickFrame, StepFatal> {
        let (frame, _) = self
            .step_frame_and_commit(false, |projection, facts| {
                let frame = projection.project_frame(facts)?;
                frame.validate().map_err(frame_fatal)?;
                Ok(frame)
            })
            .map_err(|fatal| self.poison_failed_step(fatal))?;
        Ok(frame)
    }

    /// 同一次 CommitTick transaction 产出的 public-runtime frame 与 commit evidence。
    /// Escrow verifier 使用该证据；会话不额外保留整局历史诊断状态。
    pub fn step_frame_with_commit_evidence(
        &mut self,
    ) -> Result<(TickFrame, crate::session::pipeline::TickCommitEvidence), StepFatal> {
        let (frame, evidence) = self
            .step_frame_and_commit(true, |projection, facts| {
                let frame = projection.project_frame(facts)?;
                frame.validate().map_err(frame_fatal)?;
                Ok(frame)
            })
            .map_err(|fatal| self.poison_failed_step(fatal))?;
        let evidence = evidence.expect("commit evidence was requested at the authoritative entry");
        Ok((frame, evidence))
    }

    /// 生产入口：基于同一版冻结 candidate 构造并验证 frame 后才安装权威状态。
    /// step_frame 接轨件 4：runtime 分叉（与 `step()` 的 failure.rs:892
    /// 分叉同开关同语义——缺省 runtime、ENGINE_STEP_RUNTIME=0 回旧管线）。
    /// **Escrow verifier 面（capture_commit_evidence=true）恒走旧管线**：
    /// TickCommitEvidence 的信封域证据（receipts/envelope chains）在
    /// runtime 无等价承载（D4 登记、交接 §3.2——批 9 旧管线退役时再
    /// 裁决映射或替代方案）；生产便捷入口（step_frame 经
    /// ProtocolSession，capture=false）走 runtime 分叉。
    pub(in crate::session) fn step_frame_and_commit<P>(
        &mut self,
        capture_commit_evidence: bool,
        project: impl FnOnce(TickCandidateProjection<'_>, Vec<EventFact>) -> Result<P, StepFatal>,
    ) -> Result<(P, Option<crate::session::pipeline::TickCommitEvidence>), StepFatal> {
        self.check_step_start()?;
        if !capture_commit_evidence && crate::runtime::step_runtime_enabled() {
            return self.step_frame_and_commit_via_runtime(project);
        }
        crate::session::pipeline::execute_authoritative_tick_with_projection(
            self,
            capture_commit_evidence,
            project,
        )
    }
}

fn frame_fatal(error: super::ProtocolError) -> StepFatal {
    StepFatal::InvariantViolation {
        location: "step_frame".into(),
        description: error.to_string(),
    }
}

pub fn project_timeseries(facts: &[EventFact], snapshot: crate::Snapshot) -> TickTimeseriesPayload {
    let mut timeseries_payload = TickTimeseriesPayload {
        markets: snapshot.markets,
        active_daily_candles: snapshot.active_daily_candles,
        ..TickTimeseriesPayload::default()
    };
    for fact in facts {
        let event = &fact.event;
        match event {
            Event::AuctionTick {
                code,
                phase,
                indicative_price,
                matched_volume,
                imbalance,
                tick,
                ..
            } => {
                timeseries_payload
                    .auction_points
                    .entry(code.clone())
                    .or_default()
                    .push(AuctionPoint {
                        key: fact.key.clone(),
                        tick: *tick,
                        kind: AuctionPointKind::Indication,
                        phase: *phase,
                        indicative_price: *indicative_price,
                        matched_volume: *matched_volume,
                        imbalance: Some(*imbalance),
                    });
            }
            Event::AuctionCompleted {
                code,
                tick,
                phase,
                clearing_price,
                matched_volume,
                ..
            } => {
                timeseries_payload
                    .auction_points
                    .entry(code.clone())
                    .or_default()
                    .push(AuctionPoint {
                        key: fact.key.clone(),
                        tick: *tick,
                        phase: *phase,
                        kind: AuctionPointKind::Completion,
                        indicative_price: *clearing_price,
                        matched_volume: *matched_volume,
                        imbalance: None,
                    });
            }
            Event::PriceTick {
                code,
                tick,
                last_price,
                daily_candle,
                bids,
                asks,
                ..
            } => {
                timeseries_payload.continuous_points.insert(
                    code.clone(),
                    ContinuousPoint {
                        tick: *tick,
                        phase: crate::TradingPhase::Continuous,
                        last_price: *last_price,
                        cumulative_volume: daily_candle.volume,
                        bids: bids.clone(),
                        asks: asks.clone(),
                    },
                );
            }
            Event::DayBoundary {
                closed_daily_candles,
                ..
            } => {
                timeseries_payload.closed_daily_candles = closed_daily_candles.clone();
            }
            Event::Trade { .. }
            | Event::CivilDateAdvanced { .. }
            | Event::CompanyDisclosurePublished { .. }
            | Event::IntentRejected { .. }
            | Event::SettlementError { .. }
            | Event::OrderCanceled { .. }
            | Event::OrderAccepted { .. } => {}
        }
    }
    for points in timeseries_payload.auction_points.values_mut() {
        points.sort_by(|left, right| left.key.cmp(&right.key));
    }
    timeseries_payload
}
