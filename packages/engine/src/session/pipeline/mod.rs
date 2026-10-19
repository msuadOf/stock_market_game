//! 使用 Escrow 的权威市场 tick pipeline。
//!
//! 每种交易阶段在隔离 shadow 上准备从过期处理到提交的完整 candidate。
//! 只有唯一交易阶段 dispatcher 可以安装公共运行时权威状态；
//! ledger、settlement 与事件检查成功后执行不会失败的 CommitTick 交换。

mod account_settlement;
mod account_validation;
mod account_validation_context;
mod account_validation_driver;
mod adaptive_plan_chain;
mod authoritative_tick;
mod book_auction_boundary;
#[cfg(test)]
mod book_auction_boundary_tests;
mod candidate_commit;
mod candidate_composition;
mod commit_evidence;
mod conservation;
mod continuous_lifecycle_projection;
mod continuous_matching;
mod continuous_matching_adapter;
mod continuous_projection_transaction;
mod continuous_tick_finalizer;
mod day_end;
mod market_tick_transaction;
mod order_receipts;
mod decision_resources;
mod decision_snapshot;
mod decision_snapshot_capture;
mod envelope;
mod event_collection;
mod event_key;
mod execution_fact_producers;
#[cfg(any(test, feature = "verification-harness"))]
mod executor_perturbation;
#[cfg(test)]
mod executor_perturbation_tests;
mod fill_fact;
mod institutional_experience_projection;
mod intent_candidates;
mod ledger;
mod ledger_candidate;
mod ledger_conservation;
mod ledger_paged_map;
mod ledger_rows;
mod ledger_validation;
mod live_reservations;
mod private_stock_ledger;
pub(in crate::session) use live_reservations::{
    LiveReservations, ReservationPage, RESERVATION_PAGE_SHIFT,
};
mod local_admission;
pub(crate) mod npc_decisions;
pub(in crate::session) mod npc_split;
mod npc_state_projection;
mod npc_tick_preparation;
pub(in crate::session) mod phase;
mod price_resolution;
pub(in crate::session) mod quote_expiry;
mod read_only_start;
mod ready_ingress;
mod ready_stock_stream;
mod receipt_aggregation;
mod receipt_key;
mod retail_projection;
mod session_execution_transaction;
mod session_fact_producers;
mod settlement;
mod serial_b2_surfaces;
mod stock_execution_transaction;
mod stock_stream;
pub(in crate::session) use stock_stream::TickWorkReady;
// D5 日界安装（runtime step 的 ends_day 拍——旧管线日终迁移复用）。
pub(in crate::session) use day_end::install_runtime_day_boundary;
mod transaction_error;
pub mod transition;
pub use account_validation::{
    AccountValidationContext, AccountValidationOutput, CandidateValidationInput,
    CandidateValidationResult, EnvelopeDraft, PlaceKind, StockValidation, ValidatedOperation,
};
pub use account_validation_driver::{
    AccountValidationCheckpoint, AccountValidatorDriver, CandidateValidationOutcome,
};
pub(super) use authoritative_tick::execute_authoritative_tick;
pub(in crate::session) use authoritative_tick::execute_authoritative_tick_with_projection;
pub(in crate::session) use authoritative_tick::AuthoritativeTickCommit;
pub use commit_evidence::{AuctionFinalizerExecution, CommitEnvelopeChain, TickCommitEvidence};
pub use conservation::{FeeComponents, ReceiptDelta, ResVec};
pub use decision_resources::DecisionResourceSnapshot;
pub use decision_snapshot::{DecisionAccountInput, DecisionSnapshot, DecisionSnapshotError};
pub(in crate::session) use envelope::BorrowedTickStartEnvelope;
pub use envelope::{Envelope, EnvelopeAudit, EnvelopeOrigin};
pub use event_key::*;
#[cfg(any(test, feature = "verification-harness"))]
pub(crate) use executor_perturbation::{
    current_executor_binding, with_executor_binding, ExecutorBinding,
};
#[cfg(any(test, feature = "verification-harness"))]
pub use executor_perturbation::{
    with_executor_perturbation, CanonicalMerge, ExecutorBoundary, ExecutorOrderRecord,
    ExecutorPermutation, ExecutorPerturbation,
};
pub use intent_candidates::{
    CandidateSource, CandidateSourceLocalKey, IntentCandidate, IntentCandidateBatch,
    IntentCandidateError, IntentCandidateKey,
};
pub use ledger::{EnvelopeLedger, EnvelopeReceipt, ReceiptKind};
pub use phase::TickPhase;
pub use quote_expiry::{ExpiryOutput, ExpiryRelease};
pub use receipt_key::*;
pub(super) use retail_projection::RetailProjectionSeen;

use super::{Event, GameSession, StepFatal};

#[cfg(test)]
mod account_settlement_tests;
#[cfg(test)]
mod account_validation_context_tests;
#[cfg(test)]
mod account_validation_driver_tests;
#[cfg(test)]
mod account_validation_tests;
#[cfg(test)]
mod auction_tick_transaction_tests;
#[cfg(test)]
mod authoritative_tick_tests;
#[cfg(test)]
mod candidate_commit_tests;
#[cfg(test)]
mod candidate_composition_tests;
#[cfg(test)]
mod commit_evidence_tests;
#[cfg(test)]
mod continuous_matching_adapter_tests;
#[cfg(test)]
mod continuous_matching_tests;
#[cfg(test)]
mod continuous_projection_transaction_tests;
#[cfg(test)]
mod continuous_tick_finalizer_tests;
#[cfg(test)]
mod continuous_tick_transaction_tests;
#[cfg(test)]
mod continuous_trade_acceptance_tests;
#[cfg(test)]
mod decision_output_tests;
#[cfg(test)]
mod decision_resources_tests;
#[cfg(test)]
mod decision_snapshot_capture_tests;
#[cfg(test)]
mod decision_snapshot_tests;
#[cfg(test)]
mod envelope_tests;
#[cfg(test)]
mod event_collection_tests;
#[cfg(test)]
mod execution_fact_producers_tests;
#[cfg(test)]
mod fill_fact_tests;
#[cfg(test)]
mod initial_candidate_round_tests;
#[cfg(test)]
mod intent_candidates_tests;
#[cfg(test)]
mod ledger_conservation_tests;
#[cfg(test)]
mod ledger_receipt_tests;
#[cfg(test)]
mod ledger_state_tests;
#[cfg(test)]
mod ledger_tests;
#[cfg(test)]
mod npc_decisions_tests;
#[cfg(test)]
mod npc_decisions_quota_tests;
#[cfg(test)]
mod npc_state_projection_tests;
#[cfg(test)]
mod plan_owner_tests;
#[cfg(test)]
mod npc_tick_preparation_tests;
#[cfg(test)]
#[path = "t3_boundary_tests.rs"]
mod t3_boundary_tests;
#[cfg(test)]
mod order_single_lifecycle_tests;
#[cfg(test)]
mod plan_continuation_domain_tests;
#[cfg(test)]
mod plan_quota_dead_zone_tests;
#[cfg(test)]
mod pre_open_transaction_tests;
#[cfg(test)]
mod quote_expiry_checkpoint_tests;
#[cfg(test)]
mod quote_expiry_tests;
#[cfg(test)]
mod receipt_aggregation_tests;
#[cfg(test)]
mod receipt_key_tests;
#[cfg(test)]
mod repair_lifecycle_receipt_tests;
#[cfg(test)]
mod retail_projection_persistence_tests;
#[cfg(test)]
mod retail_projection_tests;
#[cfg(test)]
mod session_execution_transaction_tests;
#[cfg(test)]
mod session_fact_producers_tests;
#[cfg(test)]
mod settlement_tests;
#[cfg(test)]
mod spine_v0_contract_tests;
#[cfg(test)]
mod stock_execution_transaction_tests;
#[cfg(test)]
mod t4_market_data_owner_tests;
#[cfg(test)]
mod t4inc76_route_cross_batch_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod transition_tests;
/// Read-only authoritative input; prospective data belongs exclusively to the plan.
pub struct PhaseInput<'a> {
    pub session: &'a GameSession,
}

impl<'a> PhaseInput<'a> {
    pub fn of(session: &'a GameSession) -> Self {
        Self { session }
    }
}

/// Discardable local plan with an owned, authoritative prospective state.
/// T2（ADR-0042 §T2）：**单一消费式 candidate owner**——原
/// `TickShadowPlan → TickShadow → Option<GameSession>` 三层嵌套包装折叠为
/// 本结构直接持有 `Option<GameSession>`（TickShadow 层删除）；失败丢弃、
/// 消费后不可复用、restore 仅在相位事务成功路径允许，语义与原包装一致。
pub struct TickShadowPlan {
    session: Option<GameSession>,
    /// T3（ADR-0042 §9.1/ADR-0032 R3）：tick 内决策产出移交槽位——决策阶段
    /// 入口生产（DecisionMerge），执行相位 [`ready_ingress`] 经交接三检查
    /// 恰一次消费；不写回 session（跨 tick `pending_npc` 载体已退役出生产）。
    decision_output: Option<npc_tick_preparation::NpcDecisionHandover>,
    event_outbox: Vec<crate::session::protocol::EventFact>,
    // T2（ADR-0042 §T2）：receipt 事实链**单载体**——applied_receipts 自带
    // local_key；独立 receipt_keys 双存已删除（消费点一次性派生，见
    // candidate_commit::prepare_tick_shadow_plan_commit）。
    applied_receipts: Vec<EnvelopeReceipt>,
    auction_finalizers: Vec<AuctionFinalizerExecution>,
    expiry: ExpiryOutput,
    expiry_applied: bool,
    decision_resources: Option<DecisionResourceSnapshot>,
    precomputed_stock_inputs:
        Option<Result<Vec<continuous_matching::ContinuousStockInput>, StepFatal>>,
    precomputed_stock_shards: Option<
        Result<
            std::collections::BTreeMap<
                crate::StockId,
                continuous_matching::IncrementalContinuousStockCoordinator,
            >,
            StepFatal,
        >,
    >,
}

impl TickShadowPlan {
    /// 捕获隔离 candidate（原 TickShadow::capture 语义：克隆 + 冻结共享
    /// ingress；供 plan_tick 与测试构造共用）。
    pub(in crate::session) fn capture_candidate(
        session: &GameSession,
    ) -> Result<GameSession, StepFatal> {
        let mut candidate = session.clone_for_tick_shadow()?;
        candidate.freeze_shared_ingress()?;
        Ok(candidate)
    }

    pub(super) fn execute<R>(
        &mut self,
        operation: impl FnOnce(&mut GameSession) -> Result<R, StepFatal>,
    ) -> Result<R, StepFatal> {
        self.execute_typed(operation)
    }

    pub(super) fn execute_typed<R, E>(
        &mut self,
        operation: impl FnOnce(&mut GameSession) -> Result<R, E>,
    ) -> Result<R, E>
    where
        E: From<StepFatal>,
    {
        operation(self.session.as_mut().ok_or_else(Self::consumed_shadow)?)
    }

    /// Transfer the discardable candidate into a phase transaction. A failure drops it;
    /// restoring a partially changed candidate would make a failed plan committable.
    pub(super) fn take_session(&mut self) -> Result<GameSession, StepFatal> {
        self.session.take().ok_or_else(Self::consumed_shadow)
    }

    pub(super) fn restore_success(&mut self, session: GameSession) -> Result<(), StepFatal> {
        if self.session.is_some() {
            return Err(StepFatal::InvariantViolation {
                description: "phase returned a candidate to an occupied tick shadow".to_owned(),
                location: "TickShadowPlan::restore_success".to_owned(),
            });
        }
        self.session = Some(session);
        Ok(())
    }

    #[cfg(test)]
    pub(super) fn envelope_ledger(&self) -> Result<EnvelopeLedger, StepFatal> {
        Ok(self
            .session
            .as_ref()
            .ok_or_else(Self::consumed_shadow)?
            .state
            .envelope_ledger
            .clone())
    }

    pub(super) fn into_session(self) -> Result<GameSession, StepFatal> {
        self.session.ok_or_else(Self::consumed_shadow)
    }

    #[cfg(test)]
    pub(super) fn strategy_state(
        &self,
        account: crate::AccountId,
    ) -> Result<crate::strategy::StrategyState, StepFatal> {
        self.session
            .as_ref()
            .ok_or_else(Self::consumed_shadow)?
            .state
            .accounts
            .get(&account)
            .and_then(|account| account.strategy())
            .ok_or_else(|| StepFatal::InvariantViolation {
                description: format!("shadow strategy missing for account {}", account.0),
                location: "TickShadowPlan::strategy_state".to_owned(),
            })?
            .production_state()
            .map_err(|error| StepFatal::InvariantViolation {
                description: error.to_string(),
                location: "TickShadowPlan::strategy_state".to_owned(),
            })
    }

    pub(super) fn consumed_shadow_public() -> StepFatal {
        Self::consumed_shadow()
    }

    fn consumed_shadow() -> StepFatal {
        StepFatal::InvariantViolation {
            description: "tick shadow was consumed by a failed phase transaction".to_owned(),
            location: "TickShadowPlan".to_owned(),
        }
    }

    #[cfg(test)]
    pub(super) fn expiry(&self) -> &ExpiryOutput {
        &self.expiry
    }

    #[cfg(test)]
    pub(super) fn decision_resources(&self) -> Result<&DecisionResourceSnapshot, StepFatal> {
        self.decision_resources
            .as_ref()
            .ok_or_else(|| StepFatal::InvariantViolation {
                description: "SealAllocationSnapshot 缺少决策资源快照".to_owned(),
                location: "TickShadowPlan::decision_resources".to_owned(),
            })
    }
}

/// 捕获隔离 tick candidate，应用报价过期并密封账户分配资源。
/// 后续步骤在交易阶段专属事务内运行，并在最后提交 tick。
/// 入口职责声明（Sol 终审·命名统一 2026-10-13）：`plan_tick` 只完成 **D 决策
/// 阶段的普通 NPC 部分**（按账户并行完整计算→`NpcDecisionHandover` 交接件）；
/// 计划 root 的生产与闭环在 `apply_session_market_transaction` 内的
/// `capture_roots`→`drain_ready_accounts_at_capture`（post-expiry）完成，
/// E 执行/收据结算/publication/commit 随后——完整链见 docs/architecture.md。
pub fn plan_tick(input: PhaseInput<'_>) -> Result<TickShadowPlan, StepFatal> {
    input.session.require_healthy()?;
    let mut candidate = TickShadowPlan::capture_candidate(input.session)?;
    // 诊断 trace 时点修正（T3 收口组 4·Astra 二轮）：先清**上一 tick 残留**的
    // 诊断（last_retail_decisions/last_retail_order_events 是「本 tick 最新」
    // 语义的瞬态诊断，S_a 携带的是上一 tick 的），**再**生产——本 tick 生产
    // 写入的新 trace 必须随 candidate 提交可见。原实现把 clear 放在生产之后，
    // 恰好抹掉本 tick 刚产出的 trace（诊断恒空的生产缺陷；见
    // spine-merge-two-phase-t3final 台账）。
    candidate.state.last_retail_decisions.clear();
    candidate.state.last_retail_order_events.clear();
    // T3（ADR-0042 §9.2）：D(a) 唯一生产点=(a→a+1) 调用决策阶段入口
    // （ExpiryShadow 前、cutoff=freeze 之后）。pop 视界=已提交 tick（candidate
    // 于 tick 推进前捕获）；观察基=已提交 `S_a`（与旧 tail 的候选逐字节等价）。
    // 整个生产属于可丢弃 shadow：失败丢弃 candidate，权威随机流/注意力不被消耗
    // （旧日界分支同语义；归并点定名 DecisionMerge，与 ADR-0032 R1/R3 共锚点）。
    // P0 重叠恢复（throughput-attribution·tree31j 单变量 +15.9% 的终态树
    // 等价）：决策生产与只读快速路径准备**并行**——共用同一 npc_split
    //（读侧=markets/ledger/账户隔离克隆等，决策写侧=personal 域/游标等，
    // 读集不相交）。串行序改为并行后两者读到的输入逐点相同（决策不写
    // markets/ledger）；资格在并行臂按分裂前生命周期克隆判定，决策完成后
    // 用原簿复核（决策理论上不触碰生命周期簿——复核为防御性一致，不一致
    // 时丢弃准备走慢路径，与串行版语义逐点一致）。
    #[cfg(any(test, feature = "verification-harness"))]
    if executor_perturbation::perturbation_scope_active() {
        // 扰动激活：回退串行（扰动框架按串行边界纪律建账，见
        // executor_perturbation::perturbation_scope_active 文档）——与
        // 串行基线逐点一致；生产路径（无扰动）走下方并行重叠。
        let decision_output = npc_tick_preparation::produce_decision_output(&mut candidate)?;
        let mut shadow_serial = TickShadowPlan {
            session: Some(candidate),
            decision_output: Some(decision_output),
            event_outbox: Vec::new(),
            applied_receipts: Vec::new(),
            auction_finalizers: Vec::new(),
            expiry: ExpiryOutput::default(),
            expiry_applied: false,
            decision_resources: None,
            precomputed_stock_inputs: None,
            precomputed_stock_shards: None,
        };
        let read_only_serial = shadow_serial.execute(|session| {
            Ok(read_only_start::can_prepare(session).then(|| read_only_start::prepare(session)))
        })?;
        return assemble_plan_tick(shadow_serial, read_only_serial);
    }
    let (view, mut writes) = npc_split::npc_split(&mut candidate);
    let compute = view.compute.clone();
    let current_market_minute = view.current_market_minute();
    crate::verification_evidence::enter_phase(TickPhase::ExpiryShadow);
    // in_place_scope：决策生产保留在协调调用线程（T1 审计「池外协调不再被
    // 外层 compute.join 间接装入 worker」的纪律维持——决策流自带池内/池外
    // 分类），只读准备作为 spawn 任务进池并行；结果经 channel 取回。
    let (decision_output, prepared_read_only) = compute.in_place_scope(|scope| {
        let (sender, receiver) = std::sync::mpsc::channel();
        let view_ref = &view;
        scope.spawn(move |_| {
            // send 失败=决策臂已先失败返回（receiver 随 scope 丢弃）——准备
            // 结果本就不再被需要，如实忽略。
            let _ = sender.send(read_only_start::fast_path_prepare_on(view_ref));
        });
        let decision_output =
            npc_tick_preparation::produce_decision_output_at_entry(&view, &mut writes);
        // 取回准备结果：协调线程在池内时**协作让位**（try_recv+yield_work，
        // 与决策流 worker 路径同纪律——阻塞 recv 会让队尾的准备任务在单
        // worker 池上饿死）；池外用真实阻塞等待。
        let prepared_read_only = if compute.is_worker() {
            loop {
                match receiver.try_recv() {
                    Ok(prepared) => break prepared,
                    Err(std::sync::mpsc::TryRecvError::Empty) => crate::compute::yield_work(),
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        return Err(StepFatal::InvariantViolation {
                            description: "read-only prepare task ended without a result"
                                .to_owned(),
                            location: "pipeline::plan_tick".to_owned(),
                        })
                    }
                }
            }
        } else {
            receiver.recv().map_err(|_| StepFatal::InvariantViolation {
                description: "read-only prepare task ended without a result".to_owned(),
                location: "pipeline::plan_tick".to_owned(),
            })?
        };
        Ok((decision_output, prepared_read_only))
    })?;
    let decision_output = decision_output?;
    let mut read_only = prepared_read_only;
    if read_only.is_some()
        && !read_only_start::fast_path_still_eligible(
            writes.npc_order_lifecycles,
            current_market_minute,
        )
    {
        read_only = None;
    }
    drop(view);
    drop(writes);
    let mut shadow = TickShadowPlan {
        session: Some(candidate),
        decision_output: Some(decision_output),
        event_outbox: Vec::new(),
        applied_receipts: Vec::new(),
        auction_finalizers: Vec::new(),
        expiry: ExpiryOutput::default(),
        expiry_applied: false,
        decision_resources: None,
        precomputed_stock_inputs: None,
        precomputed_stock_shards: None,
    };
    assemble_plan_tick(shadow, read_only)
}

/// plan_tick 尾段：快速路径准备在场则直载（免expiry/重准备），否则慢路径
///（到期计划→资源密封）。串行回退与并行重叠两条入口共用，保证两路径
/// 尾段语义逐点一致。
fn assemble_plan_tick(
    mut shadow: TickShadowPlan,
    read_only: Option<read_only_start::ReadOnlyStart>,
) -> Result<TickShadowPlan, StepFatal> {
    // T3（ADR-0042 §T3）：投机暂存已删除——commit tail 不再生产下一
    // tick 准备；无到期报价的快速路径在 tick 起始现算（非投机）。
    if let Some(prepared) = read_only {
        prepared.hydration?;
        // 已确认没有到期报价；全部资源与输入来自同一个不可变 post-expiry 起点。
        shadow.expiry_applied = true;
        crate::verification_evidence::enter_phase(TickPhase::SealAllocationSnapshot);
        shadow.decision_resources = Some(prepared.resources?);
        // 输入错误延迟到原 DecisionAndContinuation 捕获位置，保留入口/context 的首错优先级。
        shadow.precomputed_stock_inputs = Some(prepared.stock_inputs);
        // 输入成功时 coordinator 已在准备分支完成初始化；其错误仍延迟到原
        // StockProcessing 相位（validator 之后）传播，保持首错顺序不变。
        shadow.precomputed_stock_shards = prepared.stock_shards;
    } else {
        shadow.expiry = quote_expiry::plan_expiry(&mut shadow)?;
        crate::verification_evidence::enter_phase(TickPhase::SealAllocationSnapshot);
        decision_resources::plan_allocation(&mut shadow)?;
    }
    Ok(shadow)
}

#[cfg(test)]
pub(in crate::session) fn commit_injected_plan_roots_for_test(
    authority: &mut GameSession,
    roots: crate::session::plan_chain_candidates::PlanChainOperationBatch,
) -> Vec<Event> {
    use crate::TradingPhase;

    authority
        .hydrate_or_validate_envelope_ledger()
        .expect("plan-root fixture must have a valid starting ledger");
    let phase = authority.phase();
    let mut plan = plan_tick(PhaseInput { session: authority })
    .expect("计划根 tick 的 ExpiryShadow/SealAllocationSnapshot");
    market_tick_transaction::apply_tick_shadow_market_transaction_with_roots_for_test(
        &mut plan, roots,
    )
    .expect("计划根共用市场事务");
    candidate_commit::prepare_tick_shadow_plan_commit(authority, plan)
        .expect("计划根 tick 的 CommitTick 准备")
        .commit()
        .tick
        .event_values()
}

/// a1 失败原子性验收桥（增量二十；Astra 三审「注入移到真实事务中途」）：
/// 与 [`commit_injected_plan_roots_for_test`] 同一生产事务链（plan_tick →
/// 真实市场事务 → CommitTick 准备），但市场事务的 Err 与 panic 都向调用方
/// 传播（前者 `Err`、后者沿 unwind）——供测试以失败前权威状态为基线比较
/// （业务/事件零部分提交、personal 不复活、guard 终态归还）。
#[cfg(test)]
pub(in crate::session) fn run_injected_roots_market_tick_for_test(
    authority: &mut GameSession,
    roots: crate::session::plan_chain_candidates::PlanChainOperationBatch,
) -> Result<Vec<Event>, StepFatal> {
    authority
        .hydrate_or_validate_envelope_ledger()
        .expect("plan-root fixture must have a valid starting ledger");
    let mut plan = plan_tick(PhaseInput { session: authority })
    .expect("计划根 tick 的 ExpiryShadow/SealAllocationSnapshot");
    market_tick_transaction::apply_tick_shadow_market_transaction_with_roots_for_test(
        &mut plan, roots,
    )
    .map_err(|error| error.into_fatal())?;
    Ok(candidate_commit::prepare_tick_shadow_plan_commit(authority, plan)
        .expect("计划根 tick 的 CommitTick 准备（市场事务已成功，此处无注入失败路径）")
        .commit()
        .tick
        .event_values())
}

/// Events produced by a successfully committed tick.
pub struct TickCommitResult {
    pub facts: Vec<crate::session::protocol::EventFact>,
}

impl TickCommitResult {
    pub fn events(&self) -> impl ExactSizeIterator<Item = &Event> {
        self.facts.iter().map(|fact| &fact.event)
    }
    #[cfg(test)]
    pub(in crate::session) fn event_values(&self) -> Vec<Event> {
        self.facts.iter().map(|fact| fact.event.clone()).collect()
    }
    #[cfg(test)]
    pub(in crate::session) fn into_events(self) -> Vec<Event> {
        self.facts.into_iter().map(|fact| fact.event).collect()
    }
}
#[cfg(test)]
pub(in crate::session) use npc_tick_preparation::queue_npc_for_next_tick;
#[cfg(test)]
pub(in crate::session) use npc_tick_preparation::take_staged_intents_for_runtime;
#[cfg(test)]
pub(in crate::session) use npc_tick_preparation::staged_decision;
pub(in crate::session) use npc_tick_preparation::NpcDecisionHandover;
