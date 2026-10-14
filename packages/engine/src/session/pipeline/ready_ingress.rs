//! One production boundary for requests already eligible to enter the current tick.
//! Source identity is retained for receipts, never used as a trading priority.

use super::{
    adaptive_plan_chain::AdaptivePlanChainCoordinator,
    candidate_composition::compose_projected_candidates,
    local_admission::{admit_ready_batch, AccountReceipts},
    npc_tick_preparation::{take_decision_output, NpcDecisionHandover},
    stock_stream::StockStreamNotifications,
    AccountValidatorDriver, CandidateValidationOutcome, IntentCandidate, StepFatal,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::session::plan_continuation_domain::PlanContinuationDomain;
use crate::GameSession;

#[cfg(test)]
#[path = "ready_ingress_tests.rs"]
mod tests;

pub(super) struct ReadyIngress {
    initial: Vec<IntentCandidate>,
    chain: AdaptivePlanChainCoordinator,
    notifications: StockStreamNotifications,
    receipts: AccountReceipts,
}

/// 取出排队请求会修改 tick candidate；请求隔离后，
/// 根观察与 AccountValidation/StockProcessing 准备可并发读取同一报价过期后状态。
pub(super) struct ReadyIngressSources {
    initial: Vec<IntentCandidate>,
    observed_accounts: Vec<crate::AccountId>,
}

impl ReadyIngress {
    pub(super) fn capture_sources(
        session: &mut GameSession,
        decision_output: &mut Option<NpcDecisionHandover>,
    ) -> Result<ReadyIngressSources, StepFatal> {
        // T3（ADR-0042 §9.1/ADR-0032 R1-R3）：NPC 批在本 tick 决策阶段入口
        // （DecisionMerge）生产、同 tick 消费——take_decision_output 承载交接
        // 三检查（产物完整/epoch 匹配/恰一次消费）。玩家队列在 cutoff
        // （freeze_shared_ingress）时刻已截取，此处 capture 只是冻结快照的
        // 搬运——批组装顺序不构成受理优先级，receive 先后唯一证据是 receipt
        // ordinal（ADR-0032 R4.1）。
        let consuming_tick = session.state.tick;
        let output = take_decision_output(decision_output, consuming_tick)?;
        let (npc, observed_accounts) = output.into_batch(consuming_tick)?;
        let player = session.capture_player_candidate_batch();
        let initial = compose_projected_candidates(npc, player)
            .map_err(|error| invariant(&error.to_string()))?
            .into_candidates();
        Ok(ReadyIngressSources {
            initial,
            observed_accounts,
        })
    }

    pub(super) fn first_ready_batch(
        &mut self,
        session: &mut GameSession,
    ) -> Result<Vec<IntentCandidate>, StepFatal> {
        // T3 收口（四轮·组 1）·首批交付时点计数（cfg(test)）：与 root 完成
        // 同一计数域单调递增。实现内硬断言在存量 fixture 形态下不可全量
        // 成立（直接构造 ingress/复用会话的合法路径无本 tick 完成记录），
        // 收敛顺序证明由 t3_boundary_tests 的交接见证对（空批+非空批）
        // 承载——数据依赖（capture_roots 返回值构造 ReadyIngress）是结构
        // 保证，测试见证是顺序证据。
        #[cfg(test)]
        {
            let delivery_seq = session
                .compute
                .t1_probes
                .root_handover_seq
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;
            let attempt = session
                .compute
                .t1_probes
                .queue_attempt_seq
                .load(std::sync::atomic::Ordering::SeqCst);
            session
                .compute
                .t1_probes
                .first_delivery_trace
                .lock()
                .expect("首批交付见证锁被 poison")
                .push(crate::compute::FirstDeliveryRecord {
                    tick: session.state.tick,
                    delivery_seq,
                    attempt,
                });
        }
        self.chain.block_unfinished_routes(&self.initial);
        let mut ready = std::mem::take(&mut self.initial);
        ready.extend(PlanContinuationDomain::scoped(session, |domain| {
            self.chain.next_ready_batch(domain)
        })?);
        stamp_ready_receipts(session, &mut ready)?;
        Ok(ready)
    }

    pub(super) fn into_parts(
        self,
    ) -> (
        AdaptivePlanChainCoordinator,
        StockStreamNotifications,
        AccountReceipts,
    ) {
        (self.chain, self.notifications, self.receipts)
    }
}

impl ReadyIngressSources {
    pub(super) fn capture_roots(
        self,
        session: &mut GameSession,
        roots_override: Option<PlanChainOperationBatch>,
    ) -> Result<ReadyIngress, StepFatal> {
        let mut roots = match roots_override {
            Some(roots) => roots,
            None => session.capture_ready_decision_chain_roots(&self.observed_accounts)?,
        };
        #[cfg(test)]
        // 批非空（operations/routes/root 源任一非空）=非空交接；不用
        // operations.len()（root 链非空但无即时操作的合法形态计 1）。
        let root_count = usize::from(!roots.is_empty());
        let notifications = StockStreamNotifications::new();
        let mut chain = AdaptivePlanChainCoordinator::capture_batch(session, roots)?;
        // T5（Sol T5 节点审·必修 1·root 闭环）：post-expiry 冻结观察基上**完成**
        // 全部真实 root 计算（含 personal patch 归并/错误与 panic 排空/容量
        // 等待）后才交给执行——下方返回时点即「root 结果+personal patch 完整
        // 归并后」的完成见证点。执行期旧 root-feed 通道/唤醒/等待分支已删除
        //（continuation/账户授权/「无结果≠完成」保留）。
        chain.drain_ready_accounts_at_capture(session)?;
        // T3 收口（Astra 三轮·组 1）·真实 root 完成时点见证（cfg(test)）：
        // 本返回=本 tick root 批**完整完成**（post-expiry 执行相位、闭环后）
        // 时点；first_ready_batch 经本返回值的数据依赖取得全部 root 产物。
        #[cfg(test)]
        {
            let tick = session.state.tick;
            let handover_seq = session
                .compute
                .t1_probes
                .root_handover_seq
                .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
                + 1;
            let attempt = session
                .compute
                .t1_probes
                .queue_attempt_seq
                .load(std::sync::atomic::Ordering::SeqCst);
            // 同 tick 捕获序数 + 重捕获现场（六轮：双 capture 的实际调用
            // 路径证据——首例抓栈；同 tick 同 attempt 二捕=同一生产结果
            // 重捕获的判定输入）。
            let mut trace = session
                .compute
                .t1_probes
                .root_handover_trace
                .lock()
                .expect("root 交接见证锁被 poison");
            let capture_ordinal =
                trace.iter().filter(|record| record.tick == tick).count() as u32 + 1;
            let backtrace = if capture_ordinal >= 2 && trace.iter().all(|r| r.backtrace.is_none()) {
                Some(format!("{:?}", std::backtrace::Backtrace::force_capture()))
            } else {
                None
            };
            trace.push(crate::compute::RootHandoverRecord {
                tick,
                attempt,
                roots: root_count,
                handover_seq,
                capture_ordinal,
                backtrace,
            });
        }
        Ok(ReadyIngress {
            initial: self.initial,
            chain,
            notifications,
            receipts: AccountReceipts::default(),
        })
    }
}

/// Keep admitting independent roots that become ready during account validation.
/// A route waiting for its own typed feedback remains blocked by the plan batch;
/// other accounts and stocks can join the same stock-processing wave.
pub(super) fn validate_available_ready(
    chain: &mut AdaptivePlanChainCoordinator,
    receipts: &mut AccountReceipts,
    session: &mut GameSession,
    validator: &mut AccountValidatorDriver,
    mut ready: Vec<IntentCandidate>,
    all_candidates: &mut Vec<IntentCandidate>,
) -> Result<Vec<CandidateValidationOutcome>, StepFatal> {
    let mut outcomes = Vec::new();
    loop {
        if !ready.is_empty() {
            stamp_ready_receipts(session, &mut ready)?;
            let compute = session.compute.clone();
            let admitted = compute.install(|| admit_ready_batch(ready, receipts))?;
            chain.block_unfinished_routes(&admitted);
            all_candidates.extend(admitted.iter().cloned());
            crate::verification_evidence::enter_phase(super::TickPhase::AccountValidation);
            outcomes.extend(compute.install(|| validator.consume_round(admitted))?);
        }
        crate::verification_evidence::enter_phase(super::TickPhase::DecisionAndContinuation);
        ready = PlanContinuationDomain::scoped(session, |domain| chain.next_ready_batch(domain))?;
        if ready.is_empty() {
            return Ok(outcomes);
        }
    }
}

fn stamp_ready_receipts(
    session: &mut GameSession,
    candidates: &mut [IntentCandidate],
) -> Result<(), StepFatal> {
    for candidate in candidates {
        if candidate.ingress_order().is_some() {
            continue;
        }
        if !matches!(candidate.key(), super::IntentCandidateKey::PlanChain { .. }) {
            return Err(invariant("queued candidate is missing its receive ordinal"));
        }
        let received = session
            .receive_private_intent(candidate.owner(), candidate.intent().clone())
            .map_err(|error| invariant(&error.to_string()))?;
        candidate.set_ingress_order(received.account_ordinal, received.stock_ordinal);
    }
    Ok(())
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::ready_ingress".to_owned(),
    }
}

#[cfg(test)]
mod shared_receipt_tests {
    use super::*;
    use crate::{AccountId, Intent, LimitPrice, Money, Side};

    #[test]
    fn ready_plan_stamping_shares_late_player_and_npc_receipt_source() {
        let mut session =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        let source = session.shared_ingress();
        let code = session
            .stock_id(&session.state.setup.stocks[0].code.0)
            .unwrap();
        let intent = || Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(990)),
            qty: 100,
        };
        let mut shadow = session.clone_for_tick_shadow().unwrap();
        shadow.freeze_shared_ingress().unwrap();
        source
            .enqueue_player_intent(AccountId(0), intent())
            .unwrap();
        let mut plans = vec![IntentCandidate::new(
            super::super::IntentCandidateKey::plan_chain(0),
            AccountId(0),
            intent(),
        )];
        stamp_ready_receipts(&mut shadow, &mut plans).unwrap();
        assert_eq!(plans[0].ingress_order(), Some((1, 1)));
        let npc = shadow
            .receive_private_intent(AccountId(0), intent())
            .unwrap();
        assert_eq!((npc.account_ordinal, npc.stock_ordinal), (2, 2));
    }
}
