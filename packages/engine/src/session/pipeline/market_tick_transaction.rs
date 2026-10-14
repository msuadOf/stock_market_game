//! 编排 Continuous 交易阶段的完整 candidate。
//!
//! 就绪的 NPC、玩家与计划请求共用局部受理规则、不可变资源快照、
//! 持久 AccountValidation validator 与逐股 StockProcessing shadow。
//! operation stream 排空后，ReceiptAggregation/Projection 恰好运行一次。

use super::continuous_matching::IncrementalContinuousStockCoordinator;
#[cfg(test)]
use super::AccountValidationOutput;
use super::{
    account_validation_context::build_account_validation_context,
    candidate_commit::{CandidateTickCommitResult, PreparedTickPlanCommit},
    continuous_matching::{ContinuousExecutionRound, ContinuousStockInput},
    continuous_matching_adapter::{market_data_priors, prepare_incremental_continuous_inputs},
    continuous_tick_finalizer::{
        finalize_continuous_tick, ContinuousLifecycleProjectionInput, ContinuousTickBoundary,
        ContinuousTickFinalizationContext,
    },
    plan_tick,
    ready_ingress::ReadyIngress,
    ready_stock_stream::ReadyStockStream,
    session_execution_transaction::{
        SessionExecutionTransactionError, SessionExecutionTransactionOutput,
    },
    session_fact_producers::adapt_account_validation_rejection_facts,
    stock_stream::{
        continuous_shards, detached_continuous_shard, drive_stock_stream_with_feedback,
        finish_market_shards,
    },
    AccountValidatorDriver, CandidateValidationOutcome, EnvelopeReceipt, IntentCandidate,
    IntentCandidateBatch, PhaseInput, StepFatal, TickShadowPlan,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
#[cfg(test)]
use crate::session::PlanExecutionReport;
use crate::{Event, GameSession, StockId};

#[derive(Debug, thiserror::Error)]
pub(super) enum MarketTransactionError {
    #[error("Continuous tick 的 candidate 准备失败：{0}")]
    Preparation(#[source] StepFatal),
    #[error("Continuous tick 的 StockProcessing/DerivationAudit 事务失败：{0}")]
    SessionExecution(#[from] SessionExecutionTransactionError),
    #[error("Continuous tick 的收尾失败：{0}")]
    Finalization(#[source] StepFatal),
}

impl From<StepFatal> for MarketTransactionError {
    fn from(error: StepFatal) -> Self {
        Self::Preparation(error)
    }
}

impl MarketTransactionError {
    pub(super) fn into_fatal(self) -> StepFatal {
        super::transaction_error::into_fatal(self, "pipeline::market_tick_transaction")
    }
}

pub(super) struct MarketTransactionOutput {
    #[cfg(test)]
    pub(super) candidates: IntentCandidateBatch,
    #[cfg(test)]
    pub(super) validation: AccountValidationOutput,
    pub(super) facts: Vec<crate::session::protocol::EventFact>,
    pub(super) receipts: super::fill_fact::ReceiptBatch,
    pub(super) auction_finalizers: Vec<super::AuctionFinalizerExecution>,
    #[cfg(test)]
    pub(super) settlement: super::account_settlement::SettlementTransactionOutput,
    #[cfg(test)]
    pub(super) plan_reports: Vec<PlanExecutionReport>,
    #[cfg(test)]
    pub(super) execution_facts: Vec<super::continuous_matching::ContinuousExecutionFact>,
}

#[cfg(test)]
impl MarketTransactionOutput {
    pub(super) fn event_values(&self) -> Vec<Event> {
        self.facts.iter().map(|fact| fact.event.clone()).collect()
    }
}

/// 已完整校验的市场 tick；publication callback 成功后才允许唯一 CommitTick。
pub(super) struct PreparedMarketTick<'authority> {
    commit: PreparedTickPlanCommit<'authority>,
    #[cfg(test)]
    output: MarketTransactionOutput,
}

pub(super) struct MarketTickResult {
    pub(super) commit: CandidateTickCommitResult,
    #[cfg(test)]
    pub(super) output: MarketTransactionOutput,
}

/// 为权威交易阶段 dispatcher 构建隔离的完整 Continuous tick candidate。
///
/// 调用方可以检查准备失败；candidate 校验成功后不再留有可失败工作。
#[cfg(test)]
pub(super) fn prepare_market_tick(
    authority: &mut GameSession,
) -> Result<PreparedMarketTick<'_>, MarketTransactionError> {
    prepare_market_tick_with_evidence(authority, true)
}

pub(super) fn prepare_market_tick_with_evidence(
    authority: &mut GameSession,
    capture_commit_evidence: bool,
) -> Result<PreparedMarketTick<'_>, MarketTransactionError> {
    let mut plan = match plan_tick(PhaseInput {
        session: authority,
    }) {
        Ok(plan) => plan,
        Err(error) => return Err(error.into()),
    };
    let _output = apply_tick_shadow_market_transaction(&mut plan)?;
    crate::verification_evidence::enter_phase(super::TickPhase::PreCommitValidation);
    let commit = super::candidate_commit::prepare_tick_shadow_plan_commit_with_evidence(
        authority,
        plan,
        capture_commit_evidence,
    )?;
    Ok(PreparedMarketTick {
        commit,
        #[cfg(test)]
        output: _output,
    })
}

impl<'authority> PreparedMarketTick<'authority> {
    #[cfg(test)]
    pub(super) fn evidence(&self) -> &super::TickCommitEvidence {
        self.commit.evidence()
    }

    #[cfg(test)]
    pub(super) const fn output(&self) -> &MarketTransactionOutput {
        &self.output
    }

    pub(super) fn into_commit_plan(self) -> PreparedTickPlanCommit<'authority> {
        self.commit
    }

    #[cfg(test)]
    pub(super) fn commit(self) -> MarketTickResult {
        MarketTickResult {
            commit: self.commit.commit(),
            #[cfg(test)]
            output: self.output,
        }
    }
}

/// Applies the complete Continuous continuous source stream to the owned tick shadow.
pub(super) fn apply_tick_shadow_market_transaction(
    plan: &mut TickShadowPlan,
) -> Result<MarketTransactionOutput, MarketTransactionError> {
    apply_tick_shadow_market_transaction_with_roots(plan, None)
}

#[cfg(test)]
pub(super) fn apply_tick_shadow_market_transaction_with_roots_for_test(
    plan: &mut TickShadowPlan,
    roots: PlanChainOperationBatch,
) -> Result<MarketTransactionOutput, MarketTransactionError> {
    apply_tick_shadow_market_transaction_with_roots(plan, Some(roots))
}

fn apply_tick_shadow_market_transaction_with_roots(
    plan: &mut TickShadowPlan,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<MarketTransactionOutput, MarketTransactionError> {
    let resources = plan.decision_resources.take().ok_or_else(|| {
        MarketTransactionError::Preparation(invariant("SealAllocationSnapshot 缺少决策资源快照"))
    })?;
    let preceding_receipts = std::mem::take(&mut plan.applied_receipts);
    // T3（ADR-0042 §9.1）：决策阶段产出（DecisionMerge）移交执行相位消费。
    let mut decision_output = plan.decision_output.take();
    let mut candidate = plan.take_session()?;
    let mut output = apply_session_market_transaction(
        &mut candidate,
        &mut decision_output,
        resources,
        roots_override,
        &preceding_receipts,
        plan.precomputed_stock_inputs.take(),
        plan.precomputed_stock_shards.take(),
    )?;
    plan.restore_success(candidate)?;
    // 恢复既有契约：完整审计链 = ExpiryShadow前缀（已在 plan.applied_receipts
    // 中，receipts 自带 local_key——T2 单载体）+ 本股票批 output.receipts
    // （不含前缀）。前缀只在settlement输入侧合并不复用到这里。
    plan.applied_receipts = preceding_receipts;
    // T5（T2 移交·cfg(test) 克隆分支统一）：账面累积**单一移动形态**——
    // 测试构建先为 MarketTransactionOutput 留检视副本（纯字段克隆，不经
    // from_aggregation 再派生——fill 规范化恰一次的钉不受扰动），统一走
    // append-move（消除 extend-clone/append-move 双形态漂移面）。
    #[cfg(test)]
    let inspect = (
        output.receipts.clone_for_inspect(),
        output.facts.clone(),
        output.auction_finalizers.clone(),
    );
    let mut stock_batch = std::mem::take(&mut output.receipts).into_raw();
    plan.applied_receipts.append(&mut stock_batch);
    plan.event_outbox.append(&mut output.facts);
    plan.auction_finalizers.append(&mut output.auction_finalizers);
    #[cfg(test)]
    {
        output.receipts = inspect.0;
        output.facts = inspect.1;
        output.auction_finalizers = inspect.2;
    }
    Ok(output)
}

/// T4（ADR-0042 §6-T4 收尾面）：执行相位收尾**单入口**的总批产物——原三处
/// 分散 finish（计划链/账户校验/股票 shards）+ 行情/竞价收尾结果一次交付。
struct MarketExecutionFinish {
    finish: super::continuous_matching::IncrementalContinuousStockFinish,
    boundary: ContinuousTickBoundary,
    validation: super::AccountValidationOutput,
    candidates: IntentCandidateBatch,
    preceding_facts: Vec<crate::session::protocol::EventFact>,
    consumed: super::adaptive_plan_chain::PlanChainFactConsumption,
    auction_finalizers: Vec<super::AuctionFinalizerExecution>,
    #[cfg(test)]
    plan_reports: Vec<PlanExecutionReport>,
}

/// T4 收尾单入口：计划链排空（DerivationAudit）→ 账户校验收口
/// （AccountValidation）→ 候选批归并（DerivationAudit）→ 股票 owner 行情
/// 收尾（StockProcessing）。相位见证顺序与收敛前逐点一致（迁移仅收拢
/// 调用点，不改变任何相位边界或计算顺序）。
fn finish_market_execution(
    compute: &std::sync::Arc<crate::compute::ComputeContext>,
    chain: super::adaptive_plan_chain::AdaptivePlanChainCoordinator,
    validator: AccountValidatorDriver,
    all_candidates: Vec<IntentCandidate>,
    stock_execution: std::collections::BTreeMap<StockId, IncrementalContinuousStockCoordinator>,
    candidate: &GameSession,
) -> Result<MarketExecutionFinish, MarketTransactionError> {
    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    let mut plan_completion = chain.finish()?;
    let _plan_reports = std::mem::take(&mut plan_completion.reports);
    crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
    let validation = compute.parallel(|| validator.finish());
    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    let candidates =
        IntentCandidateBatch::new(all_candidates).map_err(|error| invariant(&error.to_string()))?;
    let preceding_facts =
        adapt_account_validation_rejection_facts(&candidates, validation.results())?;

    crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
    let boundary =
        ContinuousTickBoundary::capture(candidate).map_err(MarketTransactionError::Finalization)?;
    let exchanges = candidate
        .state
        .setup
        .stocks
        .iter()
        .map(|spec| {
            candidate
                .state
                .stocks
                .id(&spec.code.0)
                .map(|id| (id, spec.exchange))
                .ok_or_else(|| invariant("股票配置不在权威目录"))
        })
        .collect::<Result<std::collections::BTreeMap<_, _>, _>>()?;
    let finish = compute
        .parallel(|| {
            compute.record(crate::compute::ComputeWork::StockFinish);
            finish_market_shards(stock_execution, boundary, &exchanges)
        })
        .map_err(|fatal| match fatal {
            // Astra T4 节点审·必修 2：行情面错误（owner 迁移前位于
            // finalize 的 Finalization 包装）恢复 Finalization 分类；
            // 其余股票收尾错误分类不变（Preparation）——不得一律改类。
            super::continuous_matching::StockFinishFatal::MarketData(error) => {
                MarketTransactionError::Finalization(error)
            }
            super::continuous_matching::StockFinishFatal::Other(error) => error.into(),
        })?;
    let auction_finalizers = finish
        .auctions
        .keys()
        .map(|code| {
            super::AuctionFinalizerExecution::new(
                *code,
                1,
                u64::from(boundary.finishes_auction),
                u64::from(boundary.ends_day),
            )
        })
        .collect();
    Ok(MarketExecutionFinish {
        finish,
        boundary,
        validation,
        candidates,
        preceding_facts,
        consumed: plan_completion.consumed,
        auction_finalizers,
        #[cfg(test)]
        plan_reports: _plan_reports,
    })
}

/// T4（Astra T4 节点审·必修 1）：typed feedback 与本股轮次校验归 owner——
/// 本股任务在完成点自行完成受理结果↔事实匹配（原
/// `ReadyStockStream::take_completed` 迁移，错误文案不变）并复用
/// `validate_execution_round` 的身份/计数一致性校验。真实账户授权与跨股
/// 依赖仍走协调者窄交接；receive 顺序不变；任务不等待其他股票。
pub(in crate::session::pipeline) fn match_continuous_feedback(
    code: &StockId,
    round: &mut ContinuousExecutionRound,
    mut expected: Vec<CandidateValidationOutcome>,
) -> Result<Vec<CandidateValidationOutcome>, StepFatal> {
    let mut outcomes = Vec::new();
    for fact in &round.facts {
        let key = fact.candidate_key();
        let index = expected
            .iter()
            .position(|outcome| outcome.candidate_key() == key)
            .ok_or_else(|| invariant("StockProcessing 返回了非待处理候选"))?;
        let outcome = expected.remove(index);
        let operation = outcome.operation().ok_or_else(|| {
            invariant("StockProcessing 返回了 AccountValidation 已拒绝的候选")
        })?;
        if super::stock_stream::operation_code(operation) != code {
            return Err(invariant("StockProcessing completion 来自其他股票"));
        }
        outcomes.push(outcome);
    }
    // Sol 节点复审·最小集 1：matcher 成功返回前确认期望清空——派发受理却无
    // 事实=typed feedback 缺失（文案沿用终态断言原文）。
    if !expected.is_empty() {
        return Err(invariant("StockProcessing 的已受理候选缺少 typed feedback"));
    }
    validate_execution_round(&outcomes, round)?;
    Ok(outcomes)
}

fn apply_session_market_transaction(
    candidate: &mut GameSession,
    decision_output: &mut Option<super::NpcDecisionHandover>,
    resources: super::DecisionResourceSnapshot,
    roots_override: Option<PlanChainOperationBatch>,
    preceding_receipts: &[EnvelopeReceipt],
    precomputed_stock_inputs: Option<Result<Vec<ContinuousStockInput>, StepFatal>>,
    precomputed_stock_shards: Option<
        Result<
            std::collections::BTreeMap<StockId, IncrementalContinuousStockCoordinator>,
            StepFatal,
        >,
    >,
) -> Result<MarketTransactionOutput, MarketTransactionError> {
    crate::verification_evidence::enter_phase(super::TickPhase::DecisionAndContinuation);
    let sources = ReadyIngress::capture_sources(candidate, decision_output)?;
    // T5（Sol T5 节点审·必修 1·root 闭环）：root 捕获+**完整排空**（post-expiry
    // 冻结观察基）在输入准备前**串行**执行——排空内联 personal patch install
    // 需要独占 candidate 写（原并发臂因 &mut 不可并行）；输入准备随后读取
    // 同一 post-expiry 基（personal 域不被输入准备消费，读集不相交）。
    let mut ingress = sources.capture_roots(candidate, roots_override)?;
    let frozen_candidate: &GameSession = candidate;
    let compute = candidate.compute.clone();
    let detached = compute.install(|| -> Result<_, StepFatal> {
        let context = build_account_validation_context(frozen_candidate)?;
        let stock_inputs = match precomputed_stock_inputs {
            Some(prepared) => prepared?,
            None => prepare_incremental_continuous_inputs(frozen_candidate)?,
        };
        let ledger = frozen_candidate.state.envelope_ledger.clone();
        let next_order_id = frozen_candidate.state.next_order_id;
        let config = frozen_candidate.state.setup.config.clone();
        Ok((context, stock_inputs, ledger, next_order_id, config))
    });
    let (context, stock_inputs, ledger, next_order_id, config) = detached?;
    // T4（行情面）：行情先验从冻结 candidate 只读构建（此时 candidate 仍是
    // 首个 join 的不可变借用结束后的同一 post-expiry 状态）。
    // Sol 终审·冗余收口：预计算 shards（read-only 快速路径已注入先验）在场时
    // **不再重复捕获**——原无条件 `market_data_priors` 在该形态下被直接丢弃；
    // 先验只在未预计算（内联初始化）路径捕获。借用序不变：read_view 借用在
    // join（首个闭包取 &mut candidate）开始前完成。
    let market_priors = if precomputed_stock_shards.is_none() {
        Some(market_data_priors(&frozen_candidate.read_view()))
    } else {
        None
    };
    let (ready, prepared) = compute.join(
        || ingress.first_ready_batch(candidate),
        || -> Result<_, StepFatal> {
            let validator =
                AccountValidatorDriver::new(resources, ledger, next_order_id, config, context)?;
            // 预计算 shards 由 read-only 准备提供，错误在此处（validator 之后）传播，
            // 与原 continuous_shards 首错位置一致；未预计算时照旧内联初始化
            // （T4：行情先验随 shard 构建注入股票 owner）。
            let stock_execution = match precomputed_stock_shards {
                Some(prepared) => prepared?,
                None => continuous_shards(
                    stock_inputs,
                    market_priors.expect("内联初始化路径必须已捕获行情先验"),
                )?,
            };
            Ok((validator, stock_execution))
        },
    );
    let mut all_candidates = Vec::new();
    let (mut validator, stock_execution) = prepared?;
    let (mut chain, notifications, mut receipts) = ingress.into_parts();
    let phase = candidate.phase();
    let mut stream = ReadyStockStream::new(
        &mut chain,
        &mut receipts,
        candidate,
        &mut validator,
        &mut all_candidates,
    );
    let (initial, expected) = stream.initial(ready?)?;
    crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
    let stock_execution = drive_stock_stream_with_feedback(
        &compute,
        stock_execution,
        initial,
        expected,
        notifications,
        |code| detached_continuous_shard(code, phase),
        match_continuous_feedback,
        |progress| stream.continuous_progress(progress),
    )?;
    // T4（收尾容器收敛）：三处分散的批 finish 调用（计划链排空/账户校验收口/
    // 股票 owner 行情收尾）收敛为单一入口；相位见证顺序与原实现逐点一致。
    let execution = finish_market_execution(
        &compute,
        chain,
        validator,
        all_candidates,
        stock_execution,
        candidate,
    )?;
    let MarketExecutionFinish {
        finish,
        boundary,
        validation,
        candidates,
        preceding_facts,
        consumed,
        auction_finalizers,
        #[cfg(test)]
        plan_reports: _plan_reports,
    } = execution;
    #[cfg(test)]
    let execution_facts = finish.execution_facts.clone();
    let SessionExecutionTransactionOutput {
        facts,
        receipts,
        settlement: _settlement,
    } = finalize_continuous_tick(
        candidate,
        finish,
        preceding_facts,
        preceding_receipts,
        ContinuousTickFinalizationContext {
            boundary,
            day_end_event_base: u64::try_from(validation.results().len())
                .map_err(|_| invariant("AccountValidation 数量超出事件身份域"))?,
            lifecycle: ContinuousLifecycleProjectionInput {
                candidates: &candidates,
                validation: &validation,
                consumed: &consumed,
            },
        },
    )?;
    candidate.state.next_order_id = validation.next_order_id_after();

    Ok(MarketTransactionOutput {
        #[cfg(test)]
        candidates,
        #[cfg(test)]
        validation,
        facts,
        receipts,
        auction_finalizers,
        #[cfg(test)]
        settlement: _settlement,
        #[cfg(test)]
        plan_reports: _plan_reports,
        #[cfg(test)]
        execution_facts,
    })
}

#[cfg(test)]
pub(super) fn apply_initial_candidate_stream(
    validator: &mut AccountValidatorDriver,
    stock_execution: &mut IncrementalContinuousStockCoordinator,
    initial: &IntentCandidateBatch,
) -> Result<Vec<ContinuousExecutionRound>, StepFatal> {
    let mut rounds = Vec::new();
    if !initial.candidates().is_empty() {
        crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
        let outcomes = validator.consume_round(initial.candidates().iter().cloned())?;
        let operations = outcomes
            .iter()
            .filter_map(|outcome| outcome.operation().cloned())
            .collect::<Vec<_>>();
        if !operations.is_empty() {
            crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
            let round = stock_execution.apply_round(operations)?;
            validate_execution_round(&outcomes, &round)?;
            rounds.push(round);
        }
    }
    Ok(rounds)
}

pub(super) fn validate_execution_round(
    outcomes: &[CandidateValidationOutcome],
    round: &ContinuousExecutionRound,
) -> Result<(), StepFatal> {
    super::continuous_matching::validate_execution_facts(&round.facts)?;
    let accepted = outcomes
        .iter()
        .filter(|outcome| outcome.operation().is_some())
        .collect::<Vec<_>>();
    let accepted_identities = accepted
        .iter()
        .map(|outcome| (outcome.candidate_key().clone(), outcome.sealed_index()))
        .collect::<std::collections::BTreeSet<_>>();
    let fact_identities = round
        .facts
        .iter()
        .map(|fact| (fact.candidate_key.clone(), fact.sealed_index))
        .collect::<std::collections::BTreeSet<_>>();
    if round.facts.len() != accepted.len()
        || accepted_identities.len() != accepted.len()
        || fact_identities != accepted_identities
    {
        return Err(invariant(
            "StockProcessing 本轮事实身份与 AccountValidation 受理的 operations 不一致",
        ));
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn validate_execution_round_for_test(
    outcomes: &[CandidateValidationOutcome],
    round: &ContinuousExecutionRound,
) -> Result<(), StepFatal> {
    validate_execution_round(outcomes, round)
}

#[cfg(test)]
pub(super) fn apply_initial_candidate_stream_for_test(
    validator: &mut AccountValidatorDriver,
    stock_execution: &mut IncrementalContinuousStockCoordinator,
    initial: &IntentCandidateBatch,
) -> Result<Vec<ContinuousExecutionRound>, StepFatal> {
    apply_initial_candidate_stream(validator, stock_execution, initial)
}

// T5（批次八十二·别名收口）：旧分裂模块时代的兼容别名（模块别名×3 与
// 旧名 re-export/类型别名×12）随测试调用方全部改名后整段删除。

#[cfg(test)]
impl MarketTransactionError {
    pub(super) fn from_source_for_test(error: impl std::error::Error + 'static) -> Self {
        Self::Preparation(super::transaction_error::into_fatal(
            error,
            "pipeline::market_tick_transaction::test",
        ))
    }
}
#[cfg(test)]
impl MarketTickResult {
    pub(super) fn into_events(self) -> Vec<Event> {
        self.commit.tick.into_events()
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::market_tick_transaction".to_owned(),
    }
}
