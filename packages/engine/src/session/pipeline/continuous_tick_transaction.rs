//! 编排 Continuous 交易阶段的完整 candidate。
//!
//! 就绪的 NPC、玩家与计划请求共用局部受理规则、不可变资源快照、
//! 持久 AccountValidation validator 与逐股 StockProcessing shadow。
//! operation stream 排空后，ReceiptAggregation/Projection 恰好运行一次。

use super::{
    account_validation_context::build_account_validation_context,
    candidate_commit::{CandidateTickCommitResult, PreparedTickPlanCommit},
    continuous_matching::ContinuousExecutionRound,
    continuous_matching_adapter::prepare_incremental_continuous_inputs,
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
        continuous_shards, detached_continuous_shard, drive_stock_stream, finish_continuous_shards,
    },
    AccountValidatorDriver, CandidateValidationOutcome, EnvelopeReceipt, IntentCandidateBatch,
    PhaseInput, StepFatal, TickShadowPlan,
};
#[cfg(test)]
use super::{continuous_matching::IncrementalContinuousStockCoordinator, AccountValidationOutput};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
#[cfg(test)]
use crate::session::PlanExecutionReport;
use crate::{Event, GameSession};

#[derive(Debug, thiserror::Error)]
pub(super) enum ContinuousTransactionError {
    #[error("Continuous tick 的 candidate 准备失败：{0}")]
    Preparation(#[source] StepFatal),
    #[error("Continuous tick 的 StockProcessing/DerivationAudit 事务失败：{0}")]
    SessionExecution(#[from] SessionExecutionTransactionError),
    #[error("Continuous tick 的收尾失败：{0}")]
    Finalization(#[source] StepFatal),
}

impl From<StepFatal> for ContinuousTransactionError {
    fn from(error: StepFatal) -> Self {
        Self::Preparation(error)
    }
}

impl ContinuousTransactionError {
    pub(super) fn into_fatal(self) -> StepFatal {
        super::transaction_error::into_fatal(self, "pipeline::continuous_tick_transaction")
    }
}

pub(super) struct ContinuousTransactionOutput {
    #[cfg(test)]
    pub(super) candidates: IntentCandidateBatch,
    #[cfg(test)]
    pub(super) validation: AccountValidationOutput,
    pub(super) events: Vec<Event>,
    pub(super) event_keys: Vec<super::EventStableKey>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    #[cfg(test)]
    pub(super) settlement: super::account_settlement::SettlementTransactionOutput,
    #[cfg(test)]
    pub(super) plan_reports: Vec<PlanExecutionReport>,
}

/// 已完整校验的 Continuous tick；只剩不会失败的 CommitTick 权威状态交换。
pub(super) struct PreparedContinuousTick<'authority> {
    commit: PreparedTickPlanCommit<'authority>,
    #[cfg(test)]
    output: ContinuousTransactionOutput,
}

pub(super) struct ContinuousTickResult {
    pub(super) commit: CandidateTickCommitResult,
    #[cfg(test)]
    pub(super) output: ContinuousTransactionOutput,
}

/// 为权威交易阶段 dispatcher 构建隔离的完整 Continuous tick candidate。
///
/// 调用方可以检查准备失败；candidate 校验成功后不再留有可失败工作。
#[cfg(test)]
pub(super) fn prepare_continuous_tick(
    authority: &mut GameSession,
) -> Result<PreparedContinuousTick<'_>, ContinuousTransactionError> {
    prepare_continuous_tick_with_evidence(authority, true)
}

pub(super) fn prepare_continuous_tick_with_evidence(
    authority: &mut GameSession,
    capture_commit_evidence: bool,
) -> Result<PreparedContinuousTick<'_>, ContinuousTransactionError> {
    let mut plan = plan_tick(PhaseInput { session: authority })?;
    let _output = apply_tick_shadow_continuous_transaction(&mut plan)?;
    crate::verification_evidence::enter_phase(super::TickPhase::PreCommitValidation);
    let commit = super::candidate_commit::prepare_tick_shadow_plan_commit_with_evidence(
        authority,
        plan,
        capture_commit_evidence,
    )?;
    Ok(PreparedContinuousTick {
        commit,
        #[cfg(test)]
        output: _output,
    })
}

impl PreparedContinuousTick<'_> {
    #[cfg(test)]
    pub(super) fn evidence(&self) -> &super::TickCommitEvidence {
        self.commit.evidence()
    }

    #[cfg(test)]
    pub(super) const fn output(&self) -> &ContinuousTransactionOutput {
        &self.output
    }

    pub(super) fn commit(self) -> ContinuousTickResult {
        ContinuousTickResult {
            commit: self.commit.commit(),
            #[cfg(test)]
            output: self.output,
        }
    }
}

/// Applies the complete Continuous continuous source stream to the owned tick shadow.
pub(super) fn apply_tick_shadow_continuous_transaction(
    plan: &mut TickShadowPlan,
) -> Result<ContinuousTransactionOutput, ContinuousTransactionError> {
    apply_tick_shadow_continuous_transaction_with_roots(plan, None)
}

#[cfg(test)]
pub(super) fn apply_tick_shadow_continuous_transaction_with_roots_for_test(
    plan: &mut TickShadowPlan,
    roots: PlanChainOperationBatch,
) -> Result<ContinuousTransactionOutput, ContinuousTransactionError> {
    apply_tick_shadow_continuous_transaction_with_roots(plan, Some(roots))
}

fn apply_tick_shadow_continuous_transaction_with_roots(
    plan: &mut TickShadowPlan,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<ContinuousTransactionOutput, ContinuousTransactionError> {
    let resources = plan.decision_resources.take().ok_or_else(|| {
        ContinuousTransactionError::Preparation(invariant(
            "SealAllocationSnapshot 缺少决策资源快照",
        ))
    })?;
    let preceding_receipts = plan.applied_receipts.clone();
    let mut candidate = plan.state.take_session()?;
    let output = apply_session_continuous_transaction(
        &mut candidate,
        resources,
        roots_override,
        &preceding_receipts,
    )?;
    plan.state.restore_success(candidate)?;
    plan.receipt_keys.extend(
        output
            .receipts
            .iter()
            .map(|receipt| receipt.local_key.clone()),
    );
    plan.applied_receipts
        .extend(output.receipts.iter().cloned());
    plan.event_outbox.extend(output.events.iter().cloned());
    plan.event_keys.extend(output.event_keys.iter().cloned());
    Ok(output)
}

fn apply_session_continuous_transaction(
    candidate: &mut GameSession,
    resources: super::DecisionResourceSnapshot,
    roots_override: Option<PlanChainOperationBatch>,
    preceding_receipts: &[EnvelopeReceipt],
) -> Result<ContinuousTransactionOutput, ContinuousTransactionError> {
    crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
    let sources = ReadyIngress::capture_sources(candidate)?;
    // 两种准备读取同一报价过期后 candidate；计划根动作须等待输入隔离完成。
    // 只有 AccountValidation/StockProcessing 输入已脱离并完成校验后，计划根动作才可修改 candidate。
    let frozen_candidate: &GameSession = candidate;
    let (ingress, detached) = rayon::join(
        || sources.capture_roots(frozen_candidate, roots_override),
        || -> Result<_, StepFatal> {
            let context = build_account_validation_context(frozen_candidate)?;
            let stock_inputs = prepare_incremental_continuous_inputs(frozen_candidate)?;
            let ledger = frozen_candidate.state.envelope_ledger.clone();
            let next_order_id = frozen_candidate.state.next_order_id;
            let config = frozen_candidate.state.setup.config.clone();
            Ok((context, stock_inputs, ledger, next_order_id, config))
        },
    );
    let mut ingress = ingress?;
    let (context, stock_inputs, ledger, next_order_id, config) = detached?;
    let (ready, prepared) = rayon::join(
        || ingress.first_ready_batch(candidate),
        || -> Result<_, StepFatal> {
            let validator =
                AccountValidatorDriver::new(resources, ledger, next_order_id, config, context)?;
            let stock_execution = continuous_shards(stock_inputs)?;
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
    let initial = stream.initial(ready?)?;
    crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
    let stock_execution = drive_stock_stream(
        stock_execution,
        initial,
        notifications,
        |code| detached_continuous_shard(code, phase),
        |progress| stream.continuous_progress(progress),
    )?;
    stream.finish()?;

    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    let mut plan_completion = chain.finish()?;
    let _plan_reports = std::mem::take(&mut plan_completion.reports);
    crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
    let validation = validator.finish();
    crate::verification_evidence::enter_phase(super::TickPhase::DerivationAudit);
    let candidates =
        IntentCandidateBatch::new(all_candidates).map_err(|error| invariant(&error.to_string()))?;
    let preceding_facts =
        adapt_account_validation_rejection_facts(&candidates, validation.results())?;

    crate::verification_evidence::enter_phase(super::TickPhase::StockProcessing);
    let boundary = ContinuousTickBoundary::capture(candidate)
        .map_err(ContinuousTransactionError::Finalization)?;
    let finish = finish_continuous_shards(stock_execution, boundary.ends_day)?;
    let SessionExecutionTransactionOutput {
        events,
        event_keys,
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
                consumed: &plan_completion.consumed,
            },
        },
    )?;
    candidate.state.next_order_id = validation.next_order_id_after();

    Ok(ContinuousTransactionOutput {
        #[cfg(test)]
        candidates,
        #[cfg(test)]
        validation,
        events,
        event_keys,
        receipts,
        #[cfg(test)]
        settlement: _settlement,
        #[cfg(test)]
        plan_reports: _plan_reports,
    })
}

#[cfg(test)]
fn apply_initial_candidate_stream(
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

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::continuous_tick_transaction".to_owned(),
    }
}
