use super::*;
use crate::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
use crate::plans::{AllocationGrant, PlanOpen, PlanOpinion, PlanTarget, Urgency};

pub(super) fn execution_fixture() -> (GameSession, PlanExecutionRequest) {
    execution_fixture_with_compute(crate::compute::ComputeContext::shared_default().unwrap())
}

/// 增量二十：wave 系 a1 真实事务用例改用专用小池（确定性容量/观察者形态，
/// 隔离共享默认池的环境占槽），并置 inst_count=2（AccountId(2) 作为第二 root
/// 必须真实存在）。
pub(super) fn execution_fixture_with_compute(
    compute: std::sync::Arc<crate::compute::ComputeContext>,
) -> (GameSession, PlanExecutionRequest) {
    execution_fixture_with_compute_and_insts(compute, 2)
}

/// 增量二十一：三 root 形态（F/panic/other）需要 inst_count=3。
pub(super) fn execution_fixture_with_compute_and_insts(
    compute: std::sync::Arc<crate::compute::ComputeContext>,
    inst_count: u32,
) -> (GameSession, PlanExecutionRequest) {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = inst_count;
    let mut session = GameSession::new_with_compute(setup, 47, compute).unwrap();
    let code = StockId(0);
    let plan_id = session
        .state
        .plans
        .create(PlanOpen {
            account: AccountId(1),
            code: code.clone(),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(100),
            opinion: PlanOpinion {
                signal_score_bp: 3_000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    (
        session,
        PlanExecutionRequest {
            plan_id,
            allocation: AllocationGrant {
                plan_id,
                code,
                allocated_cash: Money::from_cents(1_000_000),
                constraint: None,
            },
            decision: QuoteDecision {
                action: QuoteAction::Submit {
                    price: Money::from_cents(900),
                    qty: 100,
                },
                reason: QuoteReason::OppositeQuoteProbe,
            },
            trading_day: 0,
        },
    )
}
