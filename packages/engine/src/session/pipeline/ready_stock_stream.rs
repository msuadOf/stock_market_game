//! 每只股票返回 typed StockProcessing 结果后立即推进依赖计划路由。
//! AccountValidation 与计划状态留在 tick coordinator；workers 只拥有各自股票 shadows。

use super::{
    adaptive_plan_chain::AdaptivePlanChainCoordinator,
    continuous_matching::ContinuousExecutionRound,
    local_admission::AccountReceipts,
    ready_ingress::validate_available_ready,
    stock_stream::{operation_code, operation_owner, StockStreamProgress},
    AccountValidatorDriver, CandidateValidationOutcome, IntentCandidate, IntentCandidateKey,
    StepFatal, ValidatedOperation,
};
use crate::session::plan_continuation_domain::PlanContinuationDomain;
use crate::{GameSession, StockId};
use std::collections::{BTreeMap, BTreeSet};

/// 进给协定的 typed 回复：就绪操作批（**操作与受理结果期望成对**——T4
/// Astra 必修 1：期望随批派发、匹配在 owner 完成点执行）或计划来源显式排空
/// （Complete——空 Vec 不再单独构成完成）。Sol 终审·冗余收口：原
/// `PendingCapacity`（执行期 root-wait 容量上提穿透）随 root capture 期闭环
/// 删除退役——drive 循环层的容量等待只保留 undispatched 分支（股票派发容量）。
pub(super) enum FeedOutcome {
    Operations {
        operations: Vec<ValidatedOperation>,
        outcomes: Vec<CandidateValidationOutcome>,
    },
    Complete,
}

pub(super) struct ReadyStockStream<'a> {
    chain: &'a mut AdaptivePlanChainCoordinator,
    receipts: &'a mut AccountReceipts,
    session: &'a mut GameSession,
    validator: &'a mut AccountValidatorDriver,
    candidates: &'a mut Vec<IntentCandidate>,
    /// T4（Astra 必修 1）+Sol 最小集 3：受理期望不再滞留 tick 协调者（原
    /// `pending` map 删除）——期望随操作批成对流入 owner 任务；此处保留
    /// **逐操作计数**的在飞路由面（受理 +1、该操作的事实回传 −1，减至 0 移除）
    /// ——与原「从 pending 再计算」精确等价；七十二的按股整批清除会提前
    /// 放掉同股后续排队批的路由标记（Sol：非阻断但等价表述过强），已修正。
    in_flight_routes: BTreeMap<(crate::AccountId, StockId), usize>,
}

impl<'a> ReadyStockStream<'a> {
    pub(super) fn new(
        chain: &'a mut AdaptivePlanChainCoordinator,
        receipts: &'a mut AccountReceipts,
        session: &'a mut GameSession,
        validator: &'a mut AccountValidatorDriver,
        candidates: &'a mut Vec<IntentCandidate>,
    ) -> Self {
        Self {
            chain,
            receipts,
            session,
            validator,
            candidates,
            in_flight_routes: BTreeMap::new(),
        }
    }

    pub(super) fn initial(
        &mut self,
        ready: Vec<IntentCandidate>,
    ) -> Result<(Vec<ValidatedOperation>, Vec<CandidateValidationOutcome>), StepFatal> {
        self.consume_immediately_ready(ready)
    }

    pub(super) fn continuous_progress(
        &mut self,
        progress: StockStreamProgress<'_, ContinuousExecutionRound>,
    ) -> Result<FeedOutcome, StepFatal> {
        // T4（Astra 必修 1）：typed feedback 已在本股 owner 任务内匹配并随
        // 完成回传（`outcomes`）——此处不再有协调者侧 pending 匹配；真实
        // 账户授权（validate_available_ready）与跨股依赖仍走本窄交接。
        let (stock, round, matched) = match progress {
            // Idle 三态循环（设计 §3.1③，落在最终 ValidatedOperation 层）：
            // (a) 校验就绪候选并推进拒绝反馈；(b) 非空 operations 立即交付；
            // (c) 空 → Complete（roots 含 pending 分量/operations/routes 全排空）
            // /池外容量上提；候选全被 AccountValidation 拒绝的死区形状由
            // 「继续循环」闭合——next_ready_batch(blocking) 在 4b/4c 内推进。
            StockStreamProgress::Idle => loop {
                let ready = PlanContinuationDomain::scoped(self.session, |domain| {
                    self.chain.next_ready_batch(domain)
                })?;
                let (operations, outcomes) = self.consume_immediately_ready(ready)?;
                if !operations.is_empty() {
                    return Ok(FeedOutcome::Operations {
                        operations,
                        outcomes,
                    });
                }
                if self.chain.plan_source_drained() {
                    return Ok(FeedOutcome::Complete);
                }
                continue;
            },
            StockStreamProgress::StockCompleted {
                code,
                round,
                outcomes,
            } => (code, round, outcomes),
        };
        // 逐操作计数释放：本批每个回传事实对其 (owner,stock) 计数 −1
        // （与原 pending 派生精确等价——同股后续排队批的标记不受影响）。
        for outcome in matched {
            if let Some(operation) = outcome.operation() {
                let route = (
                    operation_owner(operation),
                    operation_code(operation).clone(),
                );
                match self.in_flight_routes.get_mut(&route) {
                    Some(count) if *count > 1 => *count -= 1,
                    Some(_) => {
                        self.in_flight_routes.remove(&route);
                    }
                    None => return Err(invariant("StockProcessing 回传了未登记的在飞路由")),
                }
            }
        }
        let plans = plan_outcomes(matched);
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionAndContinuation);
        if plans.is_empty() {
            PlanContinuationDomain::scoped(self.session, |domain| {
                self.chain.project_execution_round(domain, round)
            })?;
        } else {
            PlanContinuationDomain::scoped(self.session, |domain| {
                self.chain
                    .advance_after_typed_outcomes(domain, &plans, Some(round))
            })?;
        }
        self.refresh_unfinished_routes();
        let ready = PlanContinuationDomain::scoped(self.session, |domain| {
            self.chain.next_ready_batch(domain)
        })?;
        let (operations, outcomes) = self.consume_immediately_ready(ready)?;
        Ok(FeedOutcome::Operations {
            operations,
            outcomes,
        })
    }

    fn consume_immediately_ready(
        &mut self,
        mut ready: Vec<IntentCandidate>,
    ) -> Result<(Vec<ValidatedOperation>, Vec<CandidateValidationOutcome>), StepFatal> {
        let mut operations = Vec::new();
        let mut expected = Vec::new();
        loop {
            if ready.is_empty() {
                return Ok((operations, expected));
            }
            let outcomes = validate_available_ready(
                self.chain,
                self.receipts,
                self.session,
                self.validator,
                ready,
                self.candidates,
            )?;
            let mut rejected_plans = Vec::new();
            for outcome in outcomes {
                if let Some(operation) = outcome.operation().cloned() {
                    // T4 必修 1：受理期望与操作成对流出（随批派发进 owner 任务），
                    // 协调者不再滞留 pending map（重复受理=AccountValidation
                    // 侧身份约束，此处只按流出序成对）。
                    *self
                        .in_flight_routes
                        .entry((
                            operation_owner(&operation),
                            operation_code(&operation).clone(),
                        ))
                        .or_default() += 1;
                    operations.push(operation);
                    expected.push(outcome);
                } else if matches!(
                    outcome.candidate_key(),
                    IntentCandidateKey::PlanChain { .. }
                ) {
                    rejected_plans.push(outcome);
                }
            }
            if !rejected_plans.is_empty() {
                PlanContinuationDomain::scoped(self.session, |domain| {
                    self.chain
                        .advance_after_typed_outcomes(domain, &rejected_plans, None)
                })?;
            }
            self.refresh_unfinished_routes();
            ready = PlanContinuationDomain::scoped(self.session, |domain| {
                self.chain.next_ready_batch(domain)
            })?;
        }
    }

    fn refresh_unfinished_routes(&mut self) {
        self.chain
            .replace_unfinished_routes(self.in_flight_routes.keys().copied().collect());
    }
}

fn plan_outcomes(outcomes: &[CandidateValidationOutcome]) -> Vec<CandidateValidationOutcome> {
    outcomes
        .iter()
        .filter(|outcome| {
            matches!(
                outcome.candidate_key(),
                IntentCandidateKey::PlanChain { .. }
            )
        })
        .cloned()
        .collect()
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::ready_stock_stream".to_owned(),
    }
}
