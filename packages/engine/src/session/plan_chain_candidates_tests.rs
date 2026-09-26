use super::*;
use crate::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
use crate::plans::{AllocationGrant, PlanOpen, PlanOpinion, PlanTarget, Urgency};

pub(super) fn execution_fixture() -> (GameSession, PlanExecutionRequest) {
    let mut session = GameSession::new(npc_working_quote_tests::quote_setup(0), 47).unwrap();
    let code = StockCode("600888".to_owned());
    let plan_id = session
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
