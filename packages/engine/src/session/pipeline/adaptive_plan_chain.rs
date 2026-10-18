//! 用同一 SealAllocationSnapshot 观察与增量 typed AccountValidation/StockProcessing 结果调度完整根计划。
//!
//! 本模块负责消费计划事实；ReceiptAggregation/Settlement 对每条 receipt 仅处理一次。
//! Projection 不得重复投影 consumed 中身份对应的 parent/PlanBook。
//! 命令结果来自 typed 事实，即时全成也不通过公共事件推断。

#[cfg(test)]
use super::DecisionSnapshot;
use super::{
    continuous_matching::{
        ContinuousCancelFact, ContinuousCancelRejection, ContinuousExecutionFact,
        ContinuousExecutionOutcome, ContinuousExecutionRound, ContinuousPlaceFact,
    },
    CandidateValidationOutcome, CandidateValidationResult, EnvelopeReceipt, IntentCandidate,
    IntentCandidateKey, ReceiptKind, ReceiptLocalKey, ReceiptSource, StepFatal, ValidatedOperation,
};
#[cfg(feature = "simulation-diagnostics")]
use crate::session::plan_execution::PlanCancelCause;
use crate::session::{
    plan_chain_candidates::{FrozenPlanChainObservation, PlanChainOperationBatch},
    plan_continuation_domain::PlanContinuationDomain,
    plan_execution::PlanRouteOutcome,
    OrderFillSettlement, PlanExecutionReport,
};
use crate::{
    AccountId, GameSession, Intent, LimitPrice, Money, OrderId, RejectionReason, Side, StockId,
};
use std::collections::{BTreeMap, BTreeSet};

fn accepted_price_matches(actual: Money, requested: LimitPrice) -> bool {
    match requested {
        LimitPrice::Fixed(price) => actual == price,
        LimitPrice::Highest | LimitPrice::Lowest => true,
    }
}

fn draft_price_matches(draft: &super::EnvelopeDraft, requested: LimitPrice) -> bool {
    match requested {
        LimitPrice::Fixed(price) => draft.requested_price().is_none() && draft.limit() == price,
        LimitPrice::Highest | LimitPrice::Lowest => draft.requested_price() == Some(requested),
    }
}

#[derive(Default)]
pub(super) struct PlanChainFactConsumption {
    operations: BTreeSet<(IntentCandidateKey, u64)>,
    receipts: BTreeSet<ReceiptLocalKey>,
    candidate_keys: BTreeSet<IntentCandidateKey>,
    sealed_indices: BTreeSet<u64>,
}

impl PlanChainFactConsumption {
    pub(super) fn contains_operation(&self, candidate: &IntentCandidateKey, sealed: u64) -> bool {
        self.operations.contains(&(candidate.clone(), sealed))
    }

    pub(super) fn contains_receipt(&self, receipt: &ReceiptLocalKey) -> bool {
        self.receipts.contains(receipt)
    }

    #[cfg(test)]
    pub(super) fn operation_count(&self) -> usize {
        self.operations.len()
    }

    #[cfg(test)]
    pub(super) fn receipt_count(&self) -> usize {
        self.receipts.len()
    }

    fn commit_round(&mut self, prepared: Self) {
        self.operations.extend(prepared.operations);
        self.receipts.extend(prepared.receipts);
        self.candidate_keys.extend(prepared.candidate_keys);
        self.sealed_indices.extend(prepared.sealed_indices);
    }

    fn prepare_continuous_round(
        &self,
        round: &ContinuousExecutionRound,
    ) -> Result<PlanChainFactConsumption, StepFatal> {
        let mut operation_ids = BTreeSet::new();
        let mut receipt_ids = BTreeSet::new();
        let mut sealed = BTreeSet::new();
        let mut candidates = BTreeSet::new();
        for fact in &round.facts {
            if self.candidate_keys.contains(&fact.candidate_key)
                || self.sealed_indices.contains(&fact.sealed_index)
                || !operation_ids.insert((fact.candidate_key.clone(), fact.sealed_index))
                || !sealed.insert(fact.sealed_index)
                || !candidates.insert(fact.candidate_key.clone())
            {
                return Err(invariant(
                    "duplicate candidate or sealed identity in plan projection",
                ));
            }
            validate_fact_identity(fact)?;
        }
        for receipt in &round.receipts {
            if self.receipts.contains(&receipt.local_key)
                || !receipt_ids.insert(receipt.local_key.clone())
            {
                return Err(invariant("duplicate receipt identity in plan projection"));
            }
        }
        Ok(Self {
            operations: operation_ids,
            receipts: receipt_ids,
            candidate_keys: candidates,
            sealed_indices: sealed,
        })
    }
}

pub(super) struct AdaptivePlanChainCompletion {
    pub(super) reports: Vec<PlanExecutionReport>,
    pub(super) consumed: PlanChainFactConsumption,
}

pub(super) struct AdaptivePlanChainCoordinator {
    roots: PlanChainOperationBatch,
    observation: Option<FrozenPlanChainObservation>,
    pending: Vec<IntentCandidate>,
    unfinished_routes: BTreeSet<(AccountId, StockId)>,
    #[cfg(feature = "simulation-diagnostics")]
    pending_cancel_causes: BTreeMap<IntentCandidateKey, PlanCancelCause>,
    consumed: PlanChainFactConsumption,
    exhausted: bool,
    failed: bool,
}

fn intent_code(intent: &Intent) -> &StockId {
    match intent {
        Intent::PlaceLimit { code, .. }
        | Intent::PlaceMarket { code, .. }
        | Intent::Cancel { code, .. } => code,
    }
}

impl AdaptivePlanChainCoordinator {
    #[cfg(test)]
    pub(super) fn capture_roots_before_matching(
        session: &GameSession,
        accepted_due_npc_ids: &[AccountId],
        snapshot: &DecisionSnapshot,
    ) -> Result<Self, StepFatal> {
        let roots = session.capture_decision_chain_roots(accepted_due_npc_ids, snapshot)?;
        Self::capture_batch(session, roots)
    }

    /// T5（Sol 终裁·完成边界见证）：root 源排空断言（委托 roots 批）。
    pub(super) fn roots_source_drained(&self) -> bool {
        self.roots.roots_source_drained()
    }

    /// T5（必修 1·root 闭环）：capture 期阻塞排空全部 root（委托 roots 批；
    /// 见 plan_chain_candidates::drain_ready_accounts_at_capture）。
    pub(super) fn drain_ready_accounts_at_capture(
        &mut self,
        session: &mut GameSession,
    ) -> Result<(), StepFatal> {
        self.roots.drain_ready_accounts_at_capture(session)
    }

    pub(super) fn capture_batch(
        session: &GameSession,
        roots: PlanChainOperationBatch,
    ) -> Result<Self, StepFatal> {
        let observation = if roots.is_empty() {
            None
        } else {
            Some(FrozenPlanChainObservation::capture(session)?)
        };
        Ok(Self {
            roots,
            observation,
            pending: Vec::new(),
            unfinished_routes: BTreeSet::new(),
            #[cfg(feature = "simulation-diagnostics")]
            pending_cancel_causes: BTreeMap::new(),
            consumed: PlanChainFactConsumption::default(),
            exhausted: false,
            failed: false,
        })
    }

    /// 就绪批入口（测试与生产共用）。Sol 终审·冗余收口：原三态入口
    /// （`next_ready_batch_typed`→`NextReady::PendingCapacity` 池外容量上提
    /// 穿透）随执行期 root-wait 删除退役——root 全部在 capture 期闭环，
    /// 执行期拉取不再等待 root，也不产生容量上提信号（root 决策期的容量
    /// 等待保留在 `drain_ready_accounts_at_capture` 内部）。
    pub(super) fn next_ready_batch(
        &mut self,
        domain: &mut PlanContinuationDomain<'_>,
    ) -> Result<Vec<IntentCandidate>, StepFatal> {
        self.yield_after_capture_drain(domain)
    }

    /// A plan may inspect its own live working orders while preparing an action.
    /// Requests from another account on the same stock do not block that view.
    pub(super) fn block_unfinished_routes(&mut self, candidates: &[IntentCandidate]) {
        self.unfinished_routes.extend(
            candidates
                .iter()
                .map(|candidate| (candidate.owner(), intent_code(candidate.intent()).clone())),
        );
    }

    #[cfg(test)]
    pub(super) fn clear_unfinished_routes(&mut self) {
        self.unfinished_routes.clear();
    }

    pub(super) fn replace_unfinished_routes(&mut self, routes: BTreeSet<(AccountId, StockId)>) {
        self.unfinished_routes = routes;
    }

    /// 计划来源是否显式排空（设计 §3.1③ Complete 判定：roots 含 pending 分量、
    /// operations 队列与 routes 全排空——对齐 PlanChainOperationBatch::is_empty）。
    pub(super) fn plan_source_drained(&self) -> bool {
        self.roots.is_empty()
    }

    fn yield_after_capture_drain(
        &mut self,
        domain: &mut PlanContinuationDomain<'_>,
    ) -> Result<Vec<IntentCandidate>, StepFatal> {
        self.ensure_active()?;
        let Some(observation) = self.observation.as_mut() else {
            if !self.roots.is_empty() {
                return self.fail(invariant(
                    "plan-chain roots exist without a frozen observation",
                ));
            }
            self.exhausted = true;
            return Ok(Vec::new());
        };
        let generated = match self.roots.yield_adaptive_candidates_after_capture_drain(
            domain,
            observation,
            &self.unfinished_routes,
        ) {
            Ok(candidates) => candidates,
            Err(error) => return self.fail(error),
        };
        let candidates = generated
            .into_iter()
            .map(|candidate| {
                IntentCandidate::new(
                    IntentCandidateKey::plan_chain(candidate.chain_generation_index),
                    candidate.owner,
                    candidate.intent,
                )
            })
            .collect::<Vec<_>>();
        #[cfg(feature = "simulation-diagnostics")]
        {
            for candidate in &candidates {
                let IntentCandidateKey::PlanChain {
                    chain_generation_index,
                } = candidate.key()
                else {
                    return self.fail(invariant("non-plan candidate yielded by plan chain"));
                };
                let cause = self.roots.pending_cancel_cause(*chain_generation_index);
                if matches!(candidate.intent(), Intent::Cancel { .. }) != cause.is_some() {
                    return self.fail(invariant(
                        "plan-chain cancellation lost its typed causal provenance",
                    ));
                }
                if let Some(cause) = cause {
                    self.pending_cancel_causes
                        .insert(candidate.key().clone(), cause);
                }
            }
        }
        self.exhausted = candidates.is_empty() && self.roots.is_empty();
        self.pending.extend(candidates.iter().cloned());
        Ok(candidates)
    }

    /// **continuation 写域缝（ADR-0042 §10.1，增量一百已收窄）**：本方法=
    /// 同股 continuation 计算的执行点，签名只接受窄写域视图
    /// `PlanContinuationDomain`（§10.2 验收钩子：零 `&mut GameSession`
    /// 全量借用，类型系统见证）——写本股 market（apply_changed_orders）、
    /// 计划簿（resume/materialize/parents，按账户）、收据路由（tick 级
    /// 共享）。剩余迁移缝（①本股 delta 内联 owner ②计划簿按账户分片
    /// ③多股容器退役）见 ADR-0042 §10.2；账户授权保留在本协调者
    /// （`validate_available_ready`，不经域）。
    pub(super) fn advance_after_typed_outcomes(
        &mut self,
        domain: &mut PlanContinuationDomain<'_>,
        steps: &[CandidateValidationOutcome],
        round: Option<&mut ContinuousExecutionRound>,
    ) -> Result<(), StepFatal> {
        self.ensure_active()?;
        let ordered = match self.outcomes_in_plan_identity_order(steps) {
            Ok(ordered) => ordered,
            Err(error) => return self.fail(error),
        };
        let result = self.advance_continuous_batch(domain, &ordered, round);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn advance_continuous_batch(
        &mut self,
        domain: &mut PlanContinuationDomain<'_>,
        steps: &[CandidateValidationOutcome],
        round: Option<&mut ContinuousExecutionRound>,
    ) -> Result<(), StepFatal> {
        let selected = self.validate_pending_subset(steps)?;
        let selected_keys = steps
            .iter()
            .map(|step| step.candidate_key().clone())
            .collect::<BTreeSet<_>>();
        let mut facts = BTreeMap::new();
        if let Some(round) = round.as_deref() {
            if round.facts.iter().any(|fact| {
                matches!(fact.candidate_key, IntentCandidateKey::PlanChain { .. })
                    && !selected_keys.contains(&fact.candidate_key)
            }) {
                return Err(invariant(
                    "StockProcessing 返回的计划事实缺少对应的 typed AccountValidation 结果",
                ));
            }
            for fact in round
                .facts
                .iter()
                .filter(|fact| selected_keys.contains(&fact.candidate_key))
            {
                if facts
                    .insert((fact.candidate_key.clone(), fact.sealed_index), fact)
                    .is_some()
                {
                    return Err(invariant("StockProcessing 返回了重复的计划命令事实"));
                }
            }
        }
        let mut outcomes = Vec::with_capacity(steps.len());
        for (candidate, step) in selected.iter().zip(steps) {
            let outcome = match step.result() {
                CandidateValidationResult::Rejected { reason, .. } => {
                    if step.operation().is_some() {
                        return Err(invariant("AccountValidation 拒绝的命令仍携带 operation"));
                    }
                    PlanRouteOutcome::Rejected(reason.clone())
                }
                CandidateValidationResult::Accepted { sealed_index, .. } => {
                    let operation = step
                        .operation()
                        .ok_or_else(|| invariant("AccountValidation 受理的命令缺少 operation"))?;
                    if operation.candidate_key() != candidate.key()
                        || operation.sealed_index() != *sealed_index
                    {
                        return Err(invariant(
                            "AccountValidation operation 身份与候选结果不一致",
                        ));
                    }
                    let fact = facts
                        .remove(&(candidate.key().clone(), *sealed_index))
                        .ok_or_else(|| {
                            invariant("AccountValidation 受理的命令缺少 StockProcessing 事实")
                        })?;
                    validate_command_fact(candidate, operation, fact)?;
                    route_outcome(&fact.outcome)
                }
            };
            let IntentCandidateKey::PlanChain {
                chain_generation_index,
            } = candidate.key()
            else {
                return Err(invariant("pending candidate is not a plan command"));
            };
            outcomes.push((
                candidate.owner(),
                intent_code(candidate.intent()).clone(),
                *chain_generation_index,
                outcome,
            ));
        }
        if !facts.is_empty() {
            return Err(invariant("StockProcessing 返回了额外的计划命令事实"));
        }
        if let Some(round) = round {
            self.project_execution_round(domain, round)?;
        }
        self.pending
            .retain(|candidate| !selected_keys.contains(candidate.key()));
        #[cfg(feature = "simulation-diagnostics")]
        for step in steps {
            self.pending_cancel_causes.remove(step.candidate_key());
        }
        self.roots.resume_adaptive_candidates(domain, outcomes)
    }
    fn validate_pending_subset(
        &self,
        steps: &[CandidateValidationOutcome],
    ) -> Result<Vec<IntentCandidate>, StepFatal> {
        if steps.is_empty() || steps.len() > self.pending.len() {
            return Err(invariant("typed result batch is not a pending plan subset"));
        }
        let pending = self
            .pending
            .iter()
            .map(|candidate| (candidate.key(), candidate))
            .collect::<BTreeMap<_, _>>();
        steps
            .iter()
            .map(|step| {
                if step.candidate_key() != step.result().key() {
                    return Err(invariant("AccountValidation 结果身份与候选不一致"));
                }
                pending
                    .get(step.candidate_key())
                    .cloned()
                    .cloned()
                    .ok_or_else(|| invariant("AccountValidation 结果不属于任何待处理计划候选"))
            })
            .collect()
    }

    fn outcomes_in_plan_identity_order(
        &self,
        steps: &[CandidateValidationOutcome],
    ) -> Result<Vec<CandidateValidationOutcome>, StepFatal> {
        self.validate_pending_subset(steps)?;
        let mut by_key = steps
            .iter()
            .map(|step| (step.candidate_key(), step))
            .collect::<BTreeMap<_, _>>();
        if by_key.len() != steps.len() {
            return Err(invariant("AccountValidation 返回了重复的计划链候选"));
        }
        let mut ordered = Vec::with_capacity(steps.len());
        for candidate in &self.pending {
            if let Some(step) = by_key.remove(candidate.key()) {
                ordered.push(step.clone());
            }
        }
        if !by_key.is_empty() {
            return Err(invariant("AccountValidation 结果属于其他计划链候选"));
        }
        Ok(ordered)
    }

    /// Also consumes NPC/player results before visiting the first plan root. The market and
    /// working-order view advances, while the frozen account/market decision view does not.
    /// ADR-0042 §10（增量一百）：轮次投影经窄写域视图执行（零全量借用）。
    pub(super) fn project_execution_round(
        &mut self,
        domain: &mut PlanContinuationDomain<'_>,
        round: &mut ContinuousExecutionRound,
    ) -> Result<(), StepFatal> {
        self.ensure_active()?;
        let result = self.project_round(domain, round);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn project_round(
        &mut self,
        domain: &mut PlanContinuationDomain<'_>,
        round: &mut ContinuousExecutionRound,
    ) -> Result<(), StepFatal> {
        let prepared = self.consumed.prepare_continuous_round(round)?;
        for (code, projection) in &mut round.projections {
            let delta = projection
                .market_delta
                .take()
                .ok_or_else(|| invariant("plan projection consumed stock changes twice"))?;
            domain.apply_market_delta(code, delta)?;
        }
        let mut sealed_receipts = BTreeMap::<u64, Vec<&EnvelopeReceipt>>::new();
        let mut finalizer_receipts = Vec::new();
        for receipt in &round.receipts {
            match receipt.local_key.source() {
                ReceiptSource::SealedIntent(sealed_index) => sealed_receipts
                    .entry(sealed_index)
                    .or_default()
                    .push(receipt),
                _ => finalizer_receipts.push(receipt),
            }
        }
        for fact in &round.facts {
            let plan_routed = matches!(fact.candidate_key, IntentCandidateKey::PlanChain { .. });
            #[cfg(feature = "simulation-diagnostics")]
            project_continuous_causal_start(
                domain,
                round,
                fact,
                causal_cancel_termination(&self.pending, &self.pending_cancel_causes, fact)?,
            )?;
            if let IntentCandidateKey::PlanChain {
                chain_generation_index,
            } = &fact.candidate_key
            {
                let candidate = self
                    .pending
                    .iter()
                    .find(|candidate| candidate.key() == &fact.candidate_key)
                    .ok_or_else(|| invariant("plan fact has no pending command"))?;
                self.roots.install_accepted_submit_parents(
                    domain,
                    &[(
                        candidate.owner(),
                        intent_code(candidate.intent()).clone(),
                        *chain_generation_index,
                        route_outcome(&fact.outcome),
                    )],
                )?;
            }
            match &fact.outcome {
                ContinuousExecutionOutcome::Place { fact, original_qty } => match fact {
                    ContinuousPlaceFact::Resting {
                        account,
                        code,
                        order_id,
                        side,
                        ..
                    }
                    | ContinuousPlaceFact::Filled {
                        account,
                        code,
                        order_id,
                        side,
                        ..
                    } => {
                        validate_parent_acceptance(
                            domain.plans(),
                            *account,
                            code,
                            *side,
                            *order_id,
                            *original_qty,
                        )?;
                        domain
                            .record_parent_order_submission(
                                *account,
                                code,
                                *side,
                                *order_id,
                                *original_qty,
                            )
                            .map_err(|error| invariant(&error.to_string()))?;
                        if let ContinuousPlaceFact::Resting {
                            sealed_index,
                            price,
                            remaining_qty,
                            ..
                        } = fact
                        {
                            let quote = round
                                .projections
                                .get(code)
                                .and_then(|projection| {
                                    projection.acceptance_quotes.get(sealed_index)
                                })
                                .ok_or_else(|| {
                                    invariant("resting NPC projection has no acceptance quote")
                                })?;
                            if quote.order.id != *order_id
                                || quote.order.owner != *account
                                || quote.order.side != *side
                                || quote.order.price != *price
                                || quote.order.qty != *remaining_qty
                            {
                                return Err(invariant(
                                    "resting acceptance quote disagrees with typed order fact",
                                ));
                            }
                            // Use the actual acceptance-time quote, even when a later operation
                            // in this round changes the market. Parent submission precedes this
                            // check so parent children retain their own execution horizon.
                            domain.register_npc_order_lifecycle_at_quote(
                                *account,
                                code,
                                &quote.order,
                                quote.last_price,
                                quote.best_bid,
                                quote.best_ask,
                            );
                        }
                    }
                    ContinuousPlaceFact::Rejected { .. } => {}
                },
                ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled {
                    account,
                    code,
                    order_id,
                    ..
                }) => {
                    // 计划链 continuation 拥有自己的撤销回写（resume 路径应用
                    // ChildOrderCanceled）；普通路径（对账撤单/到期/日终）才在
                    // 事实投影时立即清除计划执行腿的在途子单引用。
                    if !plan_routed {
                        domain
                            .record_parent_order_canceled(*account, code, *order_id)
                            .map_err(|error| invariant(&error.to_string()))?;
                    }
                    domain.remove_npc_order_lifecycle(*account, code, *order_id);
                }
                ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected { .. }) => {}
            }
            if let Some(receipts) = sealed_receipts.remove(&fact.sealed_index) {
                for receipt in receipts {
                    project_receipt(domain, receipt, plan_routed)?;
                }
            }
            #[cfg(feature = "simulation-diagnostics")]
            project_continuous_causal_end(domain, round, fact)?;
            // 成交已即时更新唯一 owner；后续操作读取已发生的进度，不再同步第二份母单。
        }
        // Auction/day-end finalizer receipts have no command outcome and are consumed only
        // after that finalizer actually runs. A future command source cannot appear here.
        if !sealed_receipts.is_empty() {
            return Err(invariant(
                "sealed receipt has no operation in this projection round",
            ));
        }
        for receipt in &finalizer_receipts {
            // 日终/竞价收尾释放不经过任何 continuation，直接按唯一 owner 语义回写。
            project_receipt(domain, receipt, false)?;
        }
        // Every order leaving the book has a terminal receipt. project_receipt
        // removes its lifecycle at the actual transition, including a new quote
        // filled later in this same round. No full-book scan is needed here.
        self.consumed.commit_round(prepared);
        Ok(())
    }

    pub(super) fn finish(self) -> Result<AdaptivePlanChainCompletion, StepFatal> {
        self.ensure_active()?;
        if !self.exhausted || !self.pending.is_empty() {
            return Err(invariant(
                "plan-chain coordinator finished before exhausting its roots",
            ));
        }
        let reports = self.roots.finish_adaptive()?;
        Ok(AdaptivePlanChainCompletion {
            reports,
            consumed: self.consumed,
        })
    }

    fn ensure_active(&self) -> Result<(), StepFatal> {
        if self.failed {
            Err(invariant(
                "plan-chain coordinator stopped after a typed failure",
            ))
        } else {
            Ok(())
        }
    }

    fn fail<T>(&mut self, error: StepFatal) -> Result<T, StepFatal> {
        self.failed = true;
        Err(error)
    }
}
#[cfg(feature = "simulation-diagnostics")]
fn project_continuous_causal_start(
    domain: &mut PlanContinuationDomain<'_>,
    round: &ContinuousExecutionRound,
    fact: &ContinuousExecutionFact,
    cancel_termination: crate::diagnostics::causal::Termination,
) -> Result<(), StepFatal> {
    let quotes = round.operation_quotes.get(&fact.sealed_index);
    match &fact.outcome {
        ContinuousExecutionOutcome::Place {
            fact:
                ContinuousPlaceFact::Resting {
                    account,
                    code,
                    order_id,
                    side,
                    ..
                }
                | ContinuousPlaceFact::Filled {
                    account,
                    code,
                    order_id,
                    side,
                    ..
                },
            original_qty,
        } => {
            let quotes = quotes.ok_or_else(|| {
                invariant("successful continuous place has no operation quote projection")
            })?;
            domain.causal_submitted_with_quote(
                *account,
                *order_id,
                code,
                *side,
                *original_qty,
                causal_quote(code, &quotes.before),
            );
        }
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled {
            account,
            code,
            order_id,
            remaining_qty,
            ..
        }) => {
            if quotes.is_none() {
                return Err(invariant(
                    "successful continuous cancellation has no operation quote projection",
                ));
            }
            domain.causal_terminated(
                (*account, *order_id, *remaining_qty),
                code,
                cancel_termination,
            );
        }
        ContinuousExecutionOutcome::Place {
            fact:
                ContinuousPlaceFact::Rejected {
                    account,
                    code,
                    order_id,
                    ..
                },
            original_qty,
        } => {
            let quotes = quotes.ok_or_else(|| {
                invariant("StockProcessing 拒绝的 Continuous Place 缺少操作前报价投影")
            })?;
            let mut rejection_receipts = round.receipts.iter().filter(|receipt| {
                receipt.local_key.source() == ReceiptSource::SealedIntent(fact.sealed_index)
                    && receipt.envelope.account == *account
                    && receipt.envelope.stock == *code
                    && receipt.envelope.order == *order_id
                    && receipt.kind == ReceiptKind::Reject
            });
            let receipt = rejection_receipts.next().ok_or_else(|| {
                invariant("StockProcessing 拒绝的 Continuous Place 缺少对应拒绝 receipt")
            })?;
            if rejection_receipts.next().is_some()
                || receipt.qty_before != *original_qty
                || receipt.qty_after != *original_qty
            {
                return Err(invariant(
                    "StockProcessing 拒绝的 Continuous Place 生命周期来源不唯一",
                ));
            }
            domain.causal_submitted_with_quote(
                *account,
                *order_id,
                code,
                receipt.envelope.side,
                *original_qty,
                causal_quote(code, &quotes.before),
            );
            domain.causal_terminated(
                (*account, *order_id, *original_qty),
                code,
                crate::diagnostics::causal::Termination::Aborted,
            );
        }
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected { .. }) => {
            if quotes.is_some() {
                return Err(invariant(
                    "rejected continuous operation exposed an operation quote projection",
                ));
            }
        }
    }
    Ok(())
}

#[cfg(feature = "simulation-diagnostics")]
fn causal_cancel_termination(
    pending: &[IntentCandidate],
    pending_causes: &BTreeMap<IntentCandidateKey, PlanCancelCause>,
    fact: &ContinuousExecutionFact,
) -> Result<crate::diagnostics::causal::Termination, StepFatal> {
    use crate::diagnostics::causal::Termination;

    if !matches!(fact.outcome, ContinuousExecutionOutcome::Cancel(_)) {
        return Ok(Termination::Voluntary);
    }
    let is_pending = pending
        .iter()
        .any(|candidate| candidate.key() == fact.candidate_key());
    match (
        is_pending,
        pending_causes.get(fact.candidate_key()).copied(),
    ) {
        (false, None) => Ok(Termination::Voluntary),
        (true, Some(cause)) => Ok(cause.causal_termination()),
        (true, None) => Err(invariant(
            "pending plan-chain cancellation has no typed causal provenance",
        )),
        (false, Some(_)) => Err(invariant(
            "plan-chain causal provenance belongs to a different operation",
        )),
    }
}

#[cfg(feature = "simulation-diagnostics")]
fn project_continuous_causal_end(
    domain: &mut PlanContinuationDomain<'_>,
    round: &ContinuousExecutionRound,
    fact: &ContinuousExecutionFact,
) -> Result<(), StepFatal> {
    use crate::diagnostics::causal::{CausalFactKind, Termination};

    let (code, side, incoming_order) = match &fact.outcome {
        ContinuousExecutionOutcome::Place {
            fact:
                ContinuousPlaceFact::Resting {
                    code,
                    order_id,
                    side,
                    ..
                }
                | ContinuousPlaceFact::Filled {
                    code,
                    order_id,
                    side,
                    ..
                },
            ..
        } => (code, Some(*side), Some(*order_id)),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled { code, .. }) => {
            (code, None, None)
        }
        ContinuousExecutionOutcome::Place {
            fact: ContinuousPlaceFact::Rejected { .. },
            ..
        }
        | ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected { .. }) => {
            return Ok(())
        }
    };
    let quotes = round
        .operation_quotes
        .get(&fact.sealed_index)
        .ok_or_else(|| {
            invariant("successful continuous operation lost its operation quote projection")
        })?;
    for trade in round
        .trades
        .iter()
        .filter(|trade| trade.triggering_sealed_index == fact.sealed_index)
    {
        domain.causal_record(CausalFactKind::Execution {
            code: trade.stock.clone(),
            maker: trade.trade.maker_order_id,
            taker: trade.trade.taker_order_id,
            side,
            qty: trade.trade.qty,
            price_cents: trade.trade.price.cents(),
            before: causal_quote(code, &quotes.before),
        });
    }
    if let Some(order_id) = incoming_order {
        for receipt in round.receipts.iter().filter(|receipt| {
            receipt.local_key.source() == ReceiptSource::SealedIntent(fact.sealed_index)
                && receipt.envelope.order == order_id
                && receipt.kind == ReceiptKind::Release
        }) {
            if receipt.qty_before == 0 {
                return Err(invariant(
                    "continuous market remainder release has zero quantity",
                ));
            }
            domain.causal_terminated(
                (receipt.envelope.account, order_id, receipt.qty_before),
                code,
                Termination::MarketRemainder,
            );
        }
    }
    domain.causal_record(CausalFactKind::Quote(causal_quote(code, &quotes.after)));
    Ok(())
}

#[cfg(feature = "simulation-diagnostics")]
fn causal_quote(
    code: &StockId,
    snapshot: &super::continuous_matching::ContinuousQuoteSnapshot,
) -> crate::diagnostics::causal::Quote {
    crate::diagnostics::causal::Quote {
        code: code.clone(),
        bid_cents: snapshot.bid_cents,
        ask_cents: snapshot.ask_cents,
        bid_depth: snapshot.bid_depth,
        ask_depth: snapshot.ask_depth,
    }
}

fn validate_command_fact(
    candidate: &IntentCandidate,
    operation: &ValidatedOperation,
    fact: &ContinuousExecutionFact,
) -> Result<(), StepFatal> {
    validate_fact_identity(fact)?;
    if fact.candidate_key != *candidate.key() || fact.sealed_index != operation.sealed_index() {
        return Err(invariant(
            "StockProcessing 事实与已 yield 的 candidate/sealed 身份不一致",
        ));
    }
    let valid = match (candidate.intent(), operation, &fact.outcome) {
        (
            Intent::PlaceLimit {
                code,
                side,
                price,
                qty,
            },
            ValidatedOperation::Place(draft),
            ContinuousExecutionOutcome::Place {
                fact: place,
                original_qty,
            },
        ) => {
            let (owner, stock, order) = place_identity(place);
            let details_match = match place {
                ContinuousPlaceFact::Resting {
                    side: actual_side,
                    price: actual_price,
                    ..
                } => actual_side == side && accepted_price_matches(*actual_price, *price),
                ContinuousPlaceFact::Filled {
                    side: actual_side,
                    filled_qty,
                    ..
                } => actual_side == side && filled_qty == qty,
                ContinuousPlaceFact::Rejected { .. } => true,
            };
            owner == candidate.owner()
                && stock == code
                && draft.order_id() == order
                && draft.owner() == owner
                && draft.code() == code
                && draft.side() == *side
                && draft_price_matches(draft, *price)
                && draft.qty() == *qty
                && original_qty == qty
                && fact.allocated_order_id == Some(order)
                && details_match
        }
        (
            Intent::Cancel { code, id },
            ValidatedOperation::Cancel {
                account,
                code: actual,
                order_id,
                ..
            },
            ContinuousExecutionOutcome::Cancel(cancel),
        ) => {
            let (owner, stock, order) = cancel_identity(cancel);
            owner == candidate.owner()
                && owner == *account
                && stock == code
                && actual == code
                && order == *id
                && order == *order_id
                && fact.allocated_order_id.is_none()
        }
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(invariant("StockProcessing payload 与已 yield 的命令不一致"))
    }
}
fn validate_fact_identity(fact: &ContinuousExecutionFact) -> Result<(), StepFatal> {
    let valid = match &fact.outcome {
        ContinuousExecutionOutcome::Place {
            fact: place,
            original_qty,
        } => {
            let inner_index = match place {
                ContinuousPlaceFact::Resting {
                    sealed_index,
                    remaining_qty,
                    ..
                } => {
                    if *remaining_qty == 0 || remaining_qty > original_qty {
                        return Err(invariant("invalid resting quantity in plan feedback"));
                    }
                    *sealed_index
                }
                ContinuousPlaceFact::Filled {
                    sealed_index,
                    filled_qty,
                    ..
                } => {
                    // A market order can terminate after a partial fill; its remaining
                    // reservation has a separate Release receipt. Plan commands are limits
                    // and get the stronger equality check in validate_command_fact.
                    if filled_qty > original_qty {
                        return Err(invariant(
                            "terminal fill feedback exceeds original order quantity",
                        ));
                    }
                    *sealed_index
                }
                ContinuousPlaceFact::Rejected { sealed_index, .. } => *sealed_index,
            };
            inner_index == fact.sealed_index
                && fact.allocated_order_id == Some(place_identity(place).2)
                && *original_qty > 0
        }
        ContinuousExecutionOutcome::Cancel(cancel) => {
            let inner_index = match cancel {
                ContinuousCancelFact::Canceled { sealed_index, .. }
                | ContinuousCancelFact::Rejected { sealed_index, .. } => *sealed_index,
            };
            inner_index == fact.sealed_index && fact.allocated_order_id.is_none()
        }
    };
    if valid {
        Ok(())
    } else {
        Err(invariant("StockProcessing 事实的 sealed/order 身份不一致"))
    }
}

fn place_identity(fact: &ContinuousPlaceFact) -> (AccountId, &StockId, OrderId) {
    match fact {
        ContinuousPlaceFact::Resting {
            account,
            code,
            order_id,
            ..
        }
        | ContinuousPlaceFact::Filled {
            account,
            code,
            order_id,
            ..
        }
        | ContinuousPlaceFact::Rejected {
            account,
            code,
            order_id,
            ..
        } => (*account, code, *order_id),
    }
}

fn cancel_identity(fact: &ContinuousCancelFact) -> (AccountId, &StockId, OrderId) {
    match fact {
        ContinuousCancelFact::Canceled {
            account,
            code,
            order_id,
            ..
        }
        | ContinuousCancelFact::Rejected {
            account,
            code,
            order_id,
            ..
        } => (*account, code, *order_id),
    }
}

fn route_outcome(outcome: &ContinuousExecutionOutcome) -> PlanRouteOutcome {
    match outcome {
        ContinuousExecutionOutcome::Place {
            fact:
                ContinuousPlaceFact::Resting { order_id, .. }
                | ContinuousPlaceFact::Filled { order_id, .. },
            ..
        } => PlanRouteOutcome::Accepted(*order_id),
        ContinuousExecutionOutcome::Place {
            fact: ContinuousPlaceFact::Rejected { reason, .. },
            ..
        } => PlanRouteOutcome::Rejected(reason.clone()),
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Canceled { order_id, .. }) => {
            PlanRouteOutcome::Canceled(*order_id)
        }
        ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected { reason, .. }) => {
            PlanRouteOutcome::Rejected(match reason {
                ContinuousCancelRejection::UnknownStock => RejectionReason::UnknownStock,
                ContinuousCancelRejection::OrderNotFound => RejectionReason::OrderNotFound,
                ContinuousCancelRejection::OrderAlreadyFilled => {
                    RejectionReason::OrderAlreadyFilled
                }
                ContinuousCancelRejection::NotOrderOwner => RejectionReason::NotOrderOwner,
                ContinuousCancelRejection::AuctionOrderNotCancelable => {
                    RejectionReason::AuctionOrderNotCancelable
                }
            })
        }
    }
}

fn validate_parent_acceptance(
    plans: &crate::plans::PlanBook,
    account: AccountId,
    code: &StockId,
    side: Side,
    order_id: OrderId,
    qty: u32,
) -> Result<(), StepFatal> {
    if let Some(plan) = plans.active_plan(account, code) {
        if plan.active_child_order_id() == Some(order_id) {
            if plan.direction() != side || plan.active_child_remaining_qty() != Some(qty) {
                return Err(invariant(
                    "typed plan acceptance disagrees with the unique owner",
                ));
            }
            if let crate::plans::PlanTarget::ShareCount(target) = plan.target() {
                if qty == 0 || qty > target.saturating_sub(plan.filled_qty()) {
                    return Err(invariant(
                        "typed acceptance exceeds parent remaining quantity",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn project_receipt(
    domain: &mut PlanContinuationDomain<'_>,
    receipt: &EnvelopeReceipt,
    plan_routed: bool,
) -> Result<(), StepFatal> {
    let key = &receipt.envelope;
    if matches!(receipt.kind, ReceiptKind::Release | ReceiptKind::Reject) {
        // 计划链 continuation 拥有自己的撤销回写；普通路径的释放才在这里
        // 立即清除计划执行腿的在途子单引用（唯一 owner 语义）。
        if !plan_routed {
            domain
                .record_parent_order_canceled(key.account, &key.stock, key.order)
                .map_err(|error| invariant(&error.to_string()))?;
        }
        domain.remove_npc_order_lifecycle(key.account, &key.stock, key.order);
        return Ok(());
    }
    if receipt.kind != ReceiptKind::Fill {
        return Ok(());
    }
    let qty = receipt
        .qty_before
        .checked_sub(receipt.qty_after)
        .filter(|qty| *qty > 0)
        .ok_or_else(|| invariant("plan fill receipt has no positive quantity"))?;
    // 这里是ReceiptAggregation之前的独立计划续行阶段，必须保留其局部链校验。
    let gross = receipt
        .value_after
        .sub(receipt.value_before)
        .map_err(|error| invariant(&error.to_string()))?;
    project_fill_values(domain, key, qty, gross, receipt.qty_after == 0)
}

/// 聚合之后的Auction续行直接消费同批typed事实，不再从raw计算qty/gross。
pub(super) fn project_aggregated_fill(
    session: &mut GameSession,
    receipt: &EnvelopeReceipt,
    fact: &super::fill_fact::FillFact,
) -> Result<(), StepFatal> {
    if receipt.kind != ReceiptKind::Fill
        || fact.key() != &receipt.envelope
        || fact.index() != super::ReceiptIndex(receipt.index)
    {
        return Err(invariant("聚合Fill事实与计划审计身份不一致"));
    }
    PlanContinuationDomain::scoped(session, |domain| {
        project_fill_values(
            domain,
            fact.key(),
            fact.qty(),
            fact.gross(),
            receipt.qty_after == 0,
        )
    })
}

fn project_fill_values(
    domain: &mut PlanContinuationDomain<'_>,
    key: &super::EnvelopeKey,
    qty: u32,
    _gross: crate::Money,
    terminal: bool,
) -> Result<(), StepFatal> {
    if let Some(plan) = domain.plans().active_plan(key.account, &key.stock) {
        if plan.direction() == key.side
            && plan.active_child_order_id() == Some(key.order)
            && (plan
                .active_child_remaining_qty()
                .is_none_or(|remaining| qty > remaining)
                || plan.filled_qty().checked_add(qty).is_none_or(|filled| {
                    filled
                        > match plan.target() {
                            crate::plans::PlanTarget::ShareCount(target) => target,
                            crate::plans::PlanTarget::PositionFractionBp(_) => u32::MAX,
                        }
                }))
        {
            return Err(invariant(
                "typed fill exceeds plan child or target quantity",
            ));
        }
    }
    let fill = OrderFillSettlement {
        account: key.account,
        side: key.side,
        order_id: key.order,
        qty,
        #[cfg(feature = "simulation-diagnostics")]
        gross: _gross,
    };
    domain
        .record_parent_order_fills(&key.stock, &[fill])
        .map_err(|error| invariant(&error.to_string()))?;
    if terminal {
        domain.remove_npc_order_lifecycle(key.account, &key.stock, key.order);
    }
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::adaptive_plan_chain".to_owned(),
    }
}

#[cfg(test)]
#[path = "adaptive_plan_chain_tests.rs"]
mod tests;

#[cfg(test)]
mod consumption_tests {
    use super::*;

    fn cancel_round(
        key: IntentCandidateKey,
        sealed_index: u64,
        inner_index: u64,
    ) -> ContinuousExecutionRound {
        ContinuousExecutionRound {
            facts: vec![ContinuousExecutionFact {
                candidate_key: key,
                sealed_index,
                allocated_order_id: None,
                outcome: ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
                    sealed_index: inner_index,
                    account: AccountId(1),
                    code: StockId(0),
                    order_id: OrderId(1),
                    reason: ContinuousCancelRejection::OrderNotFound,
                }),
            }],
            receipts: Vec::new(),
            trades: Vec::new(),
            projections: BTreeMap::new(),
            #[cfg(feature = "simulation-diagnostics")]
            operation_quotes: BTreeMap::new(),
        }
    }

    #[test]
    fn consumption_preparation_is_discardable_and_duplicate_precedes_payload_error() {
        let mut consumed = PlanChainFactConsumption::default();
        let round = cancel_round(IntentCandidateKey::player(0), 3, 3);
        let prepared = consumed.prepare_continuous_round(&round).unwrap();
        assert!(!consumed.contains_operation(&IntentCandidateKey::player(0), 3));
        drop(prepared);
        let prepared = consumed.prepare_continuous_round(&round).unwrap();
        consumed.commit_round(prepared);
        assert!(consumed.contains_operation(&IntentCandidateKey::player(0), 3));
        let duplicate = cancel_round(IntentCandidateKey::player(0), 4, 99);
        let error = consumed.prepare_continuous_round(&duplicate).err().unwrap();
        assert_eq!(
            error,
            invariant("duplicate candidate or sealed identity in plan projection")
        );
        assert_eq!(consumed.operation_count(), 1);
        assert!(!consumed.contains_operation(&IntentCandidateKey::player(0), 4));
    }

    #[test]
    fn consumption_payload_failure_precedes_a_later_duplicate_in_the_same_round() {
        let consumed = PlanChainFactConsumption::default();
        let mut round = cancel_round(IntentCandidateKey::player(0), 3, 99);
        round.facts.push(round.facts[0].clone());
        let error = consumed.prepare_continuous_round(&round).err().unwrap();
        assert_eq!(
            error,
            invariant("StockProcessing 事实的 sealed/order 身份不一致")
        );
        assert_eq!(consumed.operation_count(), 0);
    }
}
