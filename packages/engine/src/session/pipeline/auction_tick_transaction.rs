//! 增量集合竞价事务。
//!
//! AccountValidation 与股票拥有的 stock_processing 在 continuation 边界交替推进；coordinator 从 post-ExpiryShadow 初始化一次。
//! 请求按每股实际局部受理与适用价格时间规则执行，sealed identity 只用于事实关联。
//! stream 排空后，consuming finish 恰一次执行 AuctionTick、completion、DayEnd 与统一结算投影。

use super::{
    account_validation_context::build_account_validation_context,
    candidate_commit::{CandidateTickCommitResult, PreparedTickPlanCommit},
    plan_tick,
    ready_ingress::ReadyIngress,
    ready_stock_stream::ReadyStockStream,
    session_fact_producers::adapt_account_validation_rejection_facts,
    stock_auction::auction_day_end::{
        apply_incremental_auction_finish_with_prepared_facts_and_receipts, AuctionDayEndError,
        AuctionDayEndOutput, AuctionExecutionRound, AuctionTickBoundary,
        PreparedAuctionFinishContext,
    },
    stock_auction_adapter::prepare_incremental_auction_inputs,
    stock_stream::{
        auction_shards, detached_auction_shard, drive_stock_stream, finish_auction_shards,
    },
    AccountValidatorDriver, CandidateValidationOutcome, IntentCandidateBatch, PhaseInput,
    StepFatal, TickShadowPlan,
};
#[cfg(test)]
use super::{
    stock_auction::auction_day_end::IncrementalAuctionStockCoordinator, AccountValidationOutput,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
#[cfg(test)]
use crate::session::PlanExecutionReport;
use crate::GameSession;

#[derive(Debug, thiserror::Error)]
pub(super) enum AuctionTransactionError {
    #[error("Auction/DayEnd tick 的增量 Auction 准备失败：{0}")]
    Preparation(#[source] StepFatal),
    #[error("Auction/DayEnd tick 的增量 Auction 收尾失败：{0}")]
    Finalization(#[from] AuctionDayEndError),
}

impl From<StepFatal> for AuctionTransactionError {
    fn from(error: StepFatal) -> Self {
        Self::Preparation(error)
    }
}

impl AuctionTransactionError {
    pub(super) fn into_fatal(self) -> StepFatal {
        super::transaction_error::into_fatal(self, "pipeline::auction_tick_transaction")
    }
}

pub(super) struct AuctionTransactionOutput {
    #[cfg(test)]
    pub(super) candidates: IntentCandidateBatch,
    #[cfg(test)]
    pub(super) validation: AccountValidationOutput,
    pub(super) auction: AuctionDayEndOutput,
    #[cfg(test)]
    pub(super) plan_reports: Vec<PlanExecutionReport>,
}

pub(super) struct PreparedAuctionTick<'authority> {
    commit: PreparedTickPlanCommit<'authority>,
    #[cfg(test)]
    output: AuctionTransactionOutput,
}

pub(super) struct AuctionTickResult {
    pub(super) commit: CandidateTickCommitResult,
    #[cfg(test)]
    pub(super) output: AuctionTransactionOutput,
}

/// 构建 phase dispatcher 选定的竞价 candidate；开盘与收盘共享 ExpiryShadow–commit_tick 事务。
/// 各阶段的 completion 与 DayEnd 均在无失败点的 commit_tick 安装之前完成。
#[cfg(test)]
pub(super) fn prepare_auction_tick(
    authority: &mut GameSession,
) -> Result<PreparedAuctionTick<'_>, AuctionTransactionError> {
    prepare_auction_tick_with_evidence(authority, true)
}

pub(super) fn prepare_auction_tick_with_evidence(
    authority: &mut GameSession,
    capture_commit_evidence: bool,
) -> Result<PreparedAuctionTick<'_>, AuctionTransactionError> {
    let mut plan = plan_tick(PhaseInput { session: authority })?;
    let _output = apply_tick_shadow_auction_transaction(&mut plan)?;
    crate::verification_evidence::enter_phase(super::TickPhase::PreCommitValidation);
    let commit = super::candidate_commit::prepare_tick_shadow_plan_commit_with_evidence(
        authority,
        plan,
        capture_commit_evidence,
    )?;
    Ok(PreparedAuctionTick {
        commit,
        #[cfg(test)]
        output: _output,
    })
}

impl PreparedAuctionTick<'_> {
    #[cfg(test)]
    pub(super) fn evidence(&self) -> &super::TickCommitEvidence {
        self.commit.evidence()
    }

    pub(super) fn commit(self) -> AuctionTickResult {
        AuctionTickResult {
            commit: self.commit.commit(),
            #[cfg(test)]
            output: self.output,
        }
    }
}

pub(super) fn apply_tick_shadow_auction_transaction(
    plan: &mut TickShadowPlan,
) -> Result<AuctionTransactionOutput, AuctionTransactionError> {
    apply_tick_shadow_auction_transaction_inner(plan, None)
}

fn apply_tick_shadow_auction_transaction_inner(
    plan: &mut TickShadowPlan,
    roots_override: Option<PlanChainOperationBatch>,
) -> Result<AuctionTransactionOutput, AuctionTransactionError> {
    let resources = plan.decision_resources.take().ok_or_else(|| {
        AuctionTransactionError::Preparation(invariant("SealAllocationSnapshot 缺少决策资源快照"))
    })?;
    let preceding_receipts = plan.applied_receipts.clone();
    let mut candidate = plan.state.take_session()?;
    let output = apply_session_auction_transaction(
        &mut candidate,
        resources,
        roots_override,
        &preceding_receipts,
    )?;
    plan.state.restore_success(candidate)?;
    plan.receipt_keys.extend(
        output
            .auction
            .receipts
            .iter()
            .map(|receipt| receipt.local_key.clone()),
    );
    plan.applied_receipts
        .extend(output.auction.receipts.iter().cloned());
    plan.auction_finalizers
        .extend(output.auction.finalizer_executions.iter().cloned());
    plan.event_outbox
        .extend(output.auction.events.iter().cloned());
    plan.event_keys
        .extend(output.auction.event_keys.iter().cloned());
    Ok(output)
}

fn apply_session_auction_transaction(
    candidate: &mut GameSession,
    resources: super::DecisionResourceSnapshot,
    roots_override: Option<PlanChainOperationBatch>,
    preceding_receipts: &[super::EnvelopeReceipt],
) -> Result<AuctionTransactionOutput, AuctionTransactionError> {
    crate::verification_evidence::enter_phase(super::TickPhase::DecisionShadow);
    if !matches!(
        candidate.phase(),
        crate::TradingPhase::CallAuction | crate::TradingPhase::ClosingAuction
    ) {
        return Err(invariant("增量 Auction/DayEnd tick 要求 Auction 阶段").into());
    }
    let sources = ReadyIngress::capture_sources(candidate)?;
    // 根观察与股票/账户准备共享 post-ExpiryShadow 事实；两个分支完成后才允许计划变更 candidate。
    let frozen_candidate: &GameSession = candidate;
    let (ingress, detached) = rayon::join(
        || sources.capture_roots(frozen_candidate, roots_override),
        || -> Result<_, StepFatal> {
            let context = build_account_validation_context(frozen_candidate)?;
            let stock_inputs = prepare_incremental_auction_inputs(frozen_candidate)?;
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
            let stock_execution = auction_shards(stock_inputs)?;
            Ok((validator, stock_execution))
        },
    );
    let mut all_candidates = Vec::new();
    let (mut validator, stock_execution) = prepared?;
    let (mut chain, notifications, mut receipts) = ingress.into_parts();
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
        detached_auction_shard,
        |progress| stream.auction_progress(progress),
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
    let boundary = AuctionTickBoundary::capture(candidate)?;
    let finish = finish_auction_shards(stock_execution, boundary).map_err(|source| {
        AuctionDayEndError::Worker {
            code: crate::StockCode("<incremental>".to_owned()),
            source,
        }
    })?;
    let auction = apply_incremental_auction_finish_with_prepared_facts_and_receipts(
        candidate,
        &candidates,
        &validation,
        finish,
        PreparedAuctionFinishContext {
            preceding_facts,
            consumed: &plan_completion.consumed,
            preceding_receipts,
        },
    )?;

    Ok(AuctionTransactionOutput {
        #[cfg(test)]
        candidates,
        #[cfg(test)]
        validation,
        auction,
        #[cfg(test)]
        plan_reports: _plan_reports,
    })
}

#[cfg(test)]
pub(super) fn apply_tick_shadow_auction_transaction_with_roots_for_test(
    plan: &mut TickShadowPlan,
    roots: PlanChainOperationBatch,
) -> Result<AuctionTransactionOutput, AuctionTransactionError> {
    apply_tick_shadow_auction_transaction_inner(plan, Some(roots))
}

#[cfg(test)]
pub(super) fn apply_initial_candidate_stream(
    validator: &mut AccountValidatorDriver,
    stock_execution: &mut IncrementalAuctionStockCoordinator,
    initial: &IntentCandidateBatch,
) -> Result<Vec<AuctionExecutionRound>, StepFatal> {
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
    round: &AuctionExecutionRound,
) -> Result<(), StepFatal> {
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
            "Auction StockProcessing 事实身份与 AccountValidation 受理的 operations 不一致",
        ));
    }
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::auction_tick_transaction".to_owned(),
    }
}
