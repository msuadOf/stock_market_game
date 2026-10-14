//! ADR-0042 §10「continuation 窄写域」源码级验收（增量一百：契约落地）。
//!
//! 三层见证：
//! 1. **类型见证**（§10.2 验收钩子）：continuation 计算入口
//!    （`advance_after_typed_outcomes`/`project_execution_round`/
//!    `next_ready_batch`）签名只接受 `PlanContinuationDomain`——不再接受
//!    `&mut GameSession`。以函数项类型钉住签名：签名回退即本文件编译红。
//! 2. **窄视图见证**：域是会话同一计划簿的**窄视图**（非快照副本）——
//!    计划簿读写经域发生且对后续 scoped continuation 立即可见；账户授权
//!    等协调者保留职责不经域（域上不存在对应入口）。
//! 3. **语义见证**：经域推进的 continuation 保持「恰一次」（同批结果
//!    不可二次消费）与「无结果≠完成」（roots 未排空前
//!    `plan_source_drained` 恒假；空 Vec 不构成完成）。

use super::adaptive_plan_chain::AdaptivePlanChainCoordinator;
use super::continuous_matching::ContinuousExecutionRound;
use super::continuous_matching::IncrementalContinuousStockCoordinator;
use super::{
    account_validation_context::build_account_validation_context,
    continuous_matching_adapter::prepare_incremental_continuous_inputs, AccountValidatorDriver,
    CandidateValidationOutcome, DecisionResourceSnapshot, EnvelopeLedger, IntentCandidate,
    StepFatal,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::session::plan_continuation_domain::PlanContinuationDomain;
use crate::session::PlanExecutionRequest;
use crate::{AccountId, GameSession};

fn fixture() -> (GameSession, PlanExecutionRequest) {
    crate::session::plan_chain_candidates_tests::execution_fixture()
}

fn seal(
    session: &mut GameSession,
) -> (
    AccountValidatorDriver,
    IncrementalContinuousStockCoordinator,
) {
    session.state.envelope_ledger = EnvelopeLedger::new(
        session.state.next_receipt_base,
        session.project_live_envelopes().unwrap(),
    )
    .unwrap();
    let resources = DecisionResourceSnapshot::seal(session).unwrap();
    let validator = AccountValidatorDriver::new(
        resources,
        session.state.envelope_ledger.clone(),
        session.state.next_order_id,
        session.state.setup.config.clone(),
        build_account_validation_context(session).unwrap(),
    )
    .unwrap();
    let stock_execution = IncrementalContinuousStockCoordinator::from_post_expiry(
        prepare_incremental_continuous_inputs(session).unwrap(),
    )
    .unwrap();
    (validator, stock_execution)
}

fn coordinator(
    session: &GameSession,
    request: PlanExecutionRequest,
) -> AdaptivePlanChainCoordinator {
    let mut roots = PlanChainOperationBatch::empty();
    roots.push_execution(request);
    AdaptivePlanChainCoordinator::capture_batch(session, roots).unwrap()
}

/// 见证 1：continuation 入口只接受窄写域视图（编译期，非运行期断言）。
#[test]
fn continuation_entry_points_take_only_the_narrow_write_domain() {
    fn witness<'d>(
        _advance: fn(
            &mut AdaptivePlanChainCoordinator,
            &mut PlanContinuationDomain<'d>,
            &[CandidateValidationOutcome],
            Option<&mut ContinuousExecutionRound>,
        ) -> Result<(), StepFatal>,
        _project: fn(
            &mut AdaptivePlanChainCoordinator,
            &mut PlanContinuationDomain<'d>,
            &mut ContinuousExecutionRound,
        ) -> Result<(), StepFatal>,
        _next: fn(
            &mut AdaptivePlanChainCoordinator,
            &mut PlanContinuationDomain<'d>,
        ) -> Result<Vec<IntentCandidate>, StepFatal>,
    ) {
    }
    witness(
        AdaptivePlanChainCoordinator::advance_after_typed_outcomes,
        AdaptivePlanChainCoordinator::project_execution_round,
        AdaptivePlanChainCoordinator::next_ready_batch,
    );
}

/// 见证 2：域=会话同一计划簿的窄视图（非快照副本），跨 scoped 段一致。
#[test]
fn domain_views_the_live_plan_book_across_scoped_continuations() {
    let (mut session, request) = fixture();
    let mut chain = coordinator(&session, request);
    let owner = AccountId(1);
    let before = session.state.plans.active_plan_ids_for_account(owner).len();
    assert!(before >= 1, "fixture 必须携带至少一张活跃计划");

    // 第一段 scoped continuation：报价续行经域推进。
    let batch = PlanContinuationDomain::scoped(&mut session, |domain| {
        assert_eq!(
            domain.plans().active_plan_ids_for_account(owner).len(),
            before,
            "域必须呈现会话的同一份计划簿（窄视图，非快照）"
        );
        chain.next_ready_batch(domain)
    })
    .unwrap();
    assert_eq!(batch.len(), 1);

    // 域丢弃后 session 侧一致；第二段 scoped 域继续看到同一事实。
    assert_eq!(
        session.state.plans.active_plan_ids_for_account(owner).len(),
        before
    );
    PlanContinuationDomain::scoped(&mut session, |domain| {
        assert_eq!(
            domain.plans().active_plan_ids_for_account(owner).len(),
            before,
            "上一段经域的读写必须对后续 continuation 立即可见"
        );
        Ok(())
    })
    .unwrap();
}

/// 见证 3：经域推进保持「恰一次」与「无结果≠完成」。
#[test]
fn continuation_through_the_domain_keeps_exactly_once_and_no_result_is_not_complete() {
    let (mut session, request) = fixture();
    let mut chain = coordinator(&session, request);
    let (mut validator, mut stock_execution) = seal(&mut session);

    // 首批恰一条计划候选（经域交付）。
    let batch =
        PlanContinuationDomain::scoped(&mut session, |domain| chain.next_ready_batch(domain))
            .unwrap();
    assert_eq!(batch.len(), 1);
    // 无结果≠完成：候选仍在飞（尚未回传事实），来源不算排空。
    assert!(
        !chain.plan_source_drained(),
        "在飞计划候选存在时 plan_source_drained 必须为假"
    );

    let candidate = batch.into_iter().next().unwrap();
    let outcome = validator.consume(candidate.clone()).unwrap();
    let mut round = outcome.operation().map(|operation| {
        stock_execution
            .apply_round(vec![operation.clone()])
            .unwrap()
    });
    // 经域推进：受理结果 + StockProcessing 事实 + 轮次投影一次完成。
    PlanContinuationDomain::scoped(&mut session, |domain| {
        chain.advance_after_typed_outcomes(domain, std::slice::from_ref(&outcome), round.as_mut())
    })
    .unwrap();

    // 排空后：来源显式排空。
    let drained =
        PlanContinuationDomain::scoped(&mut session, |domain| chain.next_ready_batch(domain))
            .unwrap();
    assert!(drained.is_empty());
    assert!(chain.plan_source_drained());

    // 恰一次：同一结果批不可二次消费（探针使协调者进入 typed 失败终态，
    // 此后不得再有调用——finish 收口见证由既有用例承载，此处不重复）。
    let duplicate = PlanContinuationDomain::scoped(&mut session, |domain| {
        chain.advance_after_typed_outcomes(domain, std::slice::from_ref(&outcome), None)
    });
    assert!(duplicate.is_err(), "已消费的计划结果不得再次推进（恰一次）");
}
