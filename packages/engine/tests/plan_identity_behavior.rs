//! 仅使用既有公开 API 的身份行为负控；同一源码也在冻结旧基线运行。
use engine::plans::{PlanBook, PlanOpen};
use engine::{AccountId, OpinionSource, PlanOpinion, PlanTarget, Side, StockId, Urgency};

fn open(owner: u64) -> PlanOpen {
    PlanOpen {
        account: AccountId(owner),
        code: StockId(0),
        direction: Side::Buy,
        target: PlanTarget::ShareCount(100),
        opinion: PlanOpinion {
            signal_score_bp: 3000,
            source: OpinionSource::Blended,
        },
        confidence_bp: 8000,
        urgency: Urgency::Normal,
        horizon_trading_days: 5,
        created_trading_day: 0,
    }
}

#[test]
fn independent_accounts_first_plan_identities_do_not_collide() {
    let first = PlanBook::default().create(open(1)).unwrap();
    let second = PlanBook::default().create(open(2)).unwrap();
    assert_ne!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap(),
        "两个合法独立 writer 的首个计划必须具有不同的可合并身份"
    );
}
