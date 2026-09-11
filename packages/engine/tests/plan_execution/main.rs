//! W4-Task 24: persistent plans execute only through the real order lifecycle.

mod failures;
mod gold;

use engine::plans::quote_policy::{QuoteAction, QuoteDecision, QuoteReason};
use engine::plans::{AllocationGrant, PlanBook, PlanOpen};
use engine::session::{
    FloatAllocation, GameSession, NpcSetup, PlanExecutionRequest, SecurityCategory, SessionSetup,
    StockExchange, StockSpec,
};
use engine::{
    AccountId, CivilDate, GameConfig, Money, OpinionSource, PlanId, PlanOpinion, PlanTarget, Side,
    StockCode, StrategyParams, Urgency,
};

pub(crate) fn code() -> StockCode {
    StockCode("600101".into())
}

pub(crate) fn setup(
    float_shares: u32,
    ticks_per_day: u64,
    auction_ticks: u64,
    closing: u64,
) -> SessionSetup {
    let code = code();
    SessionSetup {
        stocks: vec![StockSpec {
            code: code.clone(),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1_000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 10_000_000,
            float_shares,
        }],
        npcs: NpcSetup {
            retail_count: 1,
            inst_count: 1,
            hot_count: 0,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: engine::RetailParams {
                arrival_rate: 0.01,
                order_size_mean: 100,
                chase_prob: 0.0,
                tick_cents: 1,
            },
            inst: engine::InstParams {
                margin: 0.05,
                order_size: 100,
            },
            hot: engine::HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 100,
            },
        },
        ticks_per_day,
        auction_ticks,
        closing_auction_ticks: closing,
        history_len: 8,
        t1_enabled: true,
        float_allocation: FloatAllocation::ByKind {
            retail: 0.0,
            inst: 1.0,
            hot: 0.0,
        },
        start_date: CivilDate::from_iso("2030-01-02").expect("fixture date is valid"),
    }
}

pub(crate) fn session(setup: SessionSetup) -> GameSession {
    GameSession::new(setup, 24).expect("plan execution fixture is valid")
}

pub(crate) fn create_plan(
    plans: &mut PlanBook,
    account: AccountId,
    side: Side,
    target_qty: u32,
) -> PlanId {
    plans
        .create(PlanOpen {
            account,
            code: code(),
            direction: side,
            target: PlanTarget::ShareCount(target_qty),
            opinion: PlanOpinion {
                signal_score_bp: if side == Side::Buy { 3_000 } else { -3_000 },
                source: OpinionSource::Blended,
            },
            confidence_bp: 8_000,
            urgency: Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .expect("fixture plan is valid")
}

pub(crate) fn request(plan_id: PlanId, side: Side, action: QuoteAction) -> PlanExecutionRequest {
    PlanExecutionRequest {
        plan_id,
        allocation: AllocationGrant {
            plan_id,
            code: code(),
            allocated_cash: Money::from_cents(10_000_000),
            constraint: None,
        },
        decision: QuoteDecision {
            action,
            reason: if side == Side::Buy {
                QuoteReason::OppositeQuoteProbe
            } else {
                QuoteReason::UrgentProtectedLimit
            },
        },
        trading_day: 0,
    }
}
