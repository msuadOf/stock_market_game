//! W4-Task 23：分离观点与执行紧迫度 + 报价决策（K5a 行 157 / K6 行 167–168）。
//!
//! 规定场景（任务 Acceptance）分布在：
//! - `gold.rs`：Patient/Normal/Urgent 分类矩阵、cheap-but-withdraw（固定估值仍看好
//!   但急跌加速 ⇒ 暂停/撤买单）、流动性下降（簿变单边/空 ⇒ Wait/保护限价，不虚构
//!   成交）、恢复三条件、紧迫度字段经显式修订更新（绝不静默）；
//! - `failures/mod.rs`：价格越涨跌停带 / 非法 tick 的类型化拒绝、不可撤阶段 Cancel
//!   被约束（记录 PendingReconsideration）、空簿不假设成交、急跌窗口缺失 =
//!   Unavailable 绝不触发、卖计划收到暂停评估被类型化拒绝。

mod failures;
mod gold;

use engine::plans::quote_policy::{ActiveQuote, BookTop, QuoteDecisionInputs};
use engine::plans::urgency::{PatienceStyle, PauseAssessment, UrgencyInputs};
use engine::plans::PlanOpen;
use engine::{AccountId, Money, OpinionSource, PlanOpinion, Side, StockCode, Urgency};

pub(crate) fn money(cents: i64) -> Money {
    Money::from_cents(cents)
}

/// 分类/暂停评估的基线输入：窗口可用但平静、无风险、期限充裕、信心 6000、
/// 非长期风格——即 Normal 的中性参照。各测试按需覆盖字段。
pub(crate) fn base_urgency_inputs() -> UrgencyInputs {
    UrgencyInputs {
        side: Side::Buy,
        return_30min_bp: Some(0),
        return_1min_bp: Some(0),
        risk_pressure_pause: false,
        adverse_selection_pause: false,
        risk_reduction_active: false,
        account_drawdown_bp: None,
        remaining_trading_days: 10,
        confidence_bp: 6000,
        style: PatienceStyle::Other,
    }
}

/// 报价决策的基线输入：买向 Normal、双边簿 1000/1010、保护限价 1020、
/// 涨跌停带 [900, 1100]、tick 10、无在途单、当前可撤。
pub(crate) fn base_quote_inputs() -> QuoteDecisionInputs {
    QuoteDecisionInputs {
        side: Side::Buy,
        urgency: Urgency::Normal,
        pause: PauseAssessment::Clear,
        book: BookTop {
            best_bid: Some(money(1000)),
            best_ask: Some(money(1010)),
        },
        protection_limit: money(1020),
        band_down: money(900),
        band_up: money(1100),
        tick: money(10),
        cage_bound: Some(money(1020)),
        desired_qty: 100,
        lot_size: 100,
        max_order_qty: 1_000_000,
        available_sell_qty: 0,
        active_order: None,
        cancellable_now: true,
    }
}

pub(crate) fn active_quote(price: i64) -> ActiveQuote {
    ActiveQuote {
        order_id: engine::OrderId(41),
        price: money(price),
        qty: 100,
    }
}

/// 一个看多的买计划开户（cheap-but-withdraw 场景的估值面保持看好）。
pub(crate) fn bullish_buy_open() -> PlanOpen {
    PlanOpen {
        account: AccountId(7),
        code: StockCode("600101".into()),
        direction: Side::Buy,
        target: engine::PlanTarget::ShareCount(1000),
        opinion: PlanOpinion {
            signal_score_bp: 2600,
            source: OpinionSource::Fundamental,
        },
        confidence_bp: 8000,
        urgency: Urgency::Normal,
        horizon_trading_days: 20,
        created_trading_day: 0,
    }
}
