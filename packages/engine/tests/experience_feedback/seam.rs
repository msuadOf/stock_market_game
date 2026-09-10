//! 走既有判断读缝的验收：不改 `behavior/decision.rs` 函数体，用
//! `decide_retail_position_with_experience` 证明经历输入改变/保留行为差异。
//! 行情与账户夹具与 tests/behavior.rs 同构（受控风格 + 固定随机流）。

use std::collections::BTreeMap;

use engine::{
    AccountRiskObservation, BehaviorMarketObservation, DecisionReason,
    EqualWeightMarketObservation, HorizonReturn, MarketView, PositionAction,
    PositionRiskObservation, PositionView, PricePathObservation, RetailExperienceState,
    RetailStyle, Rng, SelfView, StockCode, StockView, StrategyData,
    decide_retail_position_with_experience,
};

use super::{code, failed_round_trip, price};

/// 与 tests/behavior.rs 相同的受控行情输入（30 分钟窗口真实样本）。
fn path(thirty_minute: Option<f64>, five_day: Option<f64>) -> PricePathObservation {
    PricePathObservation {
        one_minute: HorizonReturn {
            requested_span: 1,
            available_span: 1,
            return_ratio: Some(0.0),
        },
        thirty_minute: HorizonReturn {
            requested_span: 30,
            available_span: thirty_minute.map_or(0, |_| 30),
            return_ratio: thirty_minute,
        },
        intraday: HorizonReturn {
            requested_span: 120,
            available_span: thirty_minute.map_or(0, |_| 120),
            return_ratio: thirty_minute,
        },
        five_day: HorizonReturn {
            requested_span: 5,
            available_span: five_day.map_or(0, |_| 5),
            return_ratio: five_day,
        },
        twenty_day: HorizonReturn {
            requested_span: 20,
            available_span: 0,
            return_ratio: None,
        },
        one_hundred_twenty_day: HorizonReturn {
            requested_span: 120,
            available_span: 0,
            return_ratio: None,
        },
        two_hundred_fifty_day: HorizonReturn {
            requested_span: 250,
            available_span: 0,
            return_ratio: None,
        },
        prior_thirty_minute_range: None,
    }
}

fn market_and_observations() -> (MarketView, BehaviorMarketObservation) {
    let code = code();
    let price_paths = BTreeMap::from([(
        code.clone(),
        path(Some(-0.03), Some(0.05)), // 回调买入触发：30 分钟跌超阈值且 5 日向上
    )]);
    let stocks = BTreeMap::from([(
        code,
        StockView {
            best_bid: Some(price(999)),
            best_ask: Some(price(1_001)),
            last_price: price(1_000),
            fundamental_value: None,
            recent_prices: vec![price(1_000); 20],
            recent_market_minute_prices: vec![],
            relative_volume: 1.0,
            order_book_imbalance: 0.0,
        },
    )]);
    (
        MarketView {
            stocks,
            tick: 500,
            market_minute: 300,
        },
        BehaviorMarketObservation {
            price_paths,
            thirty_minute_market: EqualWeightMarketObservation {
                total_stock_count: 1,
                observed_stock_count: 1,
                equal_weight_return: Some(0.0),
                advance_fraction: Some(1.0),
                decline_fraction: Some(0.0),
                unchanged_fraction: Some(0.0),
            },
        },
    )
}

fn no_position_views() -> (SelfView, AccountRiskObservation) {
    (
        SelfView {
            cash: price(10_000_000),
            positions: BTreeMap::new(),
        },
        AccountRiskObservation {
            equity: price(10_000_000),
            return_from_reference: None,
            drawdown_from_peak: None,
            positions: BTreeMap::new(),
        },
    )
}

struct FixedRng {
    value: f64,
}

impl Rng for FixedRng {
    fn next_f64(&mut self) -> f64 {
        self.value
    }

    fn next_range_u32(&mut self, _lo: u32, _hi: u32) -> u32 {
        // 单股夹具恒选第 0 只；本文件不测抽样分布。
        0
    }
}

fn strategy() -> StrategyData {
    let mut data = StrategyData::retail(1.0, 500, 0.5, 1);
    data.stop_loss_threshold = 0.05;
    data.take_profit_threshold = 0.08;
    data.dip_threshold = 0.02;
    data.max_stock_fraction = 0.40;
    data
}

#[test]
fn same_pnl_different_experience_chooses_differently_under_one_style() {
    // 两名散户损益输入完全相同（同权益参照、同市场、同账户、同风格、同随机流）；
    // 唯一差异是其中一人的真实受挫经历。既有读缝必须给出不同选择。
    let (market, observations) = market_and_observations();
    let (own, risk) = no_position_views();
    let code = code();

    let fresh = RetailExperienceState::new(price(1_000_000)).unwrap();
    let decision_fresh = decide_retail_position_with_experience(
        &strategy(),
        RetailStyle::LongTerm,
        &market,
        &own,
        &observations,
        &risk,
        &fresh,
        300,
        &mut FixedRng { value: 0.9 },
    );

    let mut scarred = RetailExperienceState::new(price(1_000_000)).unwrap();
    for round in 0..3_u64 {
        failed_round_trip(&mut scarred, &code, 10 + round * 2, round, round * 10);
    }
    assert_eq!(scarred.consecutive_failed_buys, 3);
    assert_eq!(scarred.feedback.failure_events.len(), 3);
    let decision_scarred = decide_retail_position_with_experience(
        &strategy(),
        RetailStyle::LongTerm,
        &market,
        &own,
        &observations,
        &risk,
        &scarred,
        300,
        &mut FixedRng { value: 0.9 },
    );

    assert_eq!(decision_fresh.action, PositionAction::TryBuy);
    assert_eq!(decision_fresh.reason, DecisionReason::Pullback);
    assert_eq!(decision_scarred.action, PositionAction::Watch);
    assert_eq!(decision_scarred.reason, DecisionReason::LowConfidence);
    assert_ne!(decision_fresh.action, decision_scarred.action);
}

#[test]
fn panic_exits_the_same_loss_that_long_term_holds() {
    // 同一深亏、同一经历状态：恐慌者止损离场，长期者继续持有——
    // 经历接线不得把所有人压成同一止损动作。
    let code = StockCode("600101".into());
    let (market, observations) = market_and_observations();
    let own = SelfView {
        cash: price(10_000_000),
        positions: BTreeMap::from([(
            code.clone(),
            PositionView {
                qty: 1_000,
                sellable_qty: 1_000,
                cost_price: Some(price(1_111)),
            },
        )]),
    };
    let risk = AccountRiskObservation {
        equity: price(10_000_000),
        return_from_reference: None,
        drawdown_from_peak: None,
        positions: BTreeMap::from([(
            code.clone(),
            PositionRiskObservation {
                market_value: price(1_000_000),
                unrealized_return: Some(-0.10),
                equity_weight: Some(0.10),
                drawdown_from_position_peak: None,
            },
        )]),
    };
    let mut experience = RetailExperienceState::new(price(10_000_000)).unwrap();
    failed_round_trip(&mut experience, &code, 1, 0, 0);
    failed_round_trip(&mut experience, &code, 3, 0, 10);

    let panic = decide_retail_position_with_experience(
        &strategy(),
        RetailStyle::Panic,
        &market,
        &own,
        &observations,
        &risk,
        &experience,
        300,
        &mut FixedRng { value: 0.9 },
    );
    let long_term = decide_retail_position_with_experience(
        &strategy(),
        RetailStyle::LongTerm,
        &market,
        &own,
        &observations,
        &risk,
        &experience,
        300,
        &mut FixedRng { value: 0.9 },
    );

    assert_eq!(panic.action, PositionAction::Exit);
    assert_eq!(panic.reason, DecisionReason::PositionRisk);
    assert_eq!(long_term.action, PositionAction::Hold);
    assert_eq!(long_term.reason, DecisionReason::PositionRisk);
    assert_eq!(long_term.executable_delta_shares, 0);
}
