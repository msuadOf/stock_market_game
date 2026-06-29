//! engine strategy 三策略 + 工厂集成测试（TDD 红绿循环）。
use engine::strategy::{MarketView, StockView, Rng};
use engine::account::StockCode;
use engine::money::Money;
use std::collections::BTreeMap;

// 固定种子 mock Rng：返回预定序列。
struct SeqRng { vals: Vec<f64>, idx: usize, u32s: Vec<u32>, uidx: usize }
impl SeqRng {
    fn new_f64(v: f64) -> Self { SeqRng { vals: vec![v], idx: 0, u32s: vec![], uidx: 0 } }
}
impl Rng for SeqRng {
    fn next_f64(&mut self) -> f64 {
        let v = self.vals[self.idx.min(self.vals.len() - 1)];
        self.idx += 1;
        v
    }
    fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32 {
        let v = self.u32s.get(self.uidx).copied().unwrap_or(lo);
        self.uidx += 1;
        if hi <= lo { lo } else { v }
    }
}

fn one_stock_view(last: i64, v: Option<i64>) -> MarketView {
    let mut stocks = BTreeMap::new();
    stocks.insert(StockCode("600101".to_string()), StockView {
        best_bid: Some(Money::from_cents(last - 1)),
        best_ask: Some(Money::from_cents(last + 1)),
        last_price: Money::from_cents(last),
        fundamental_value: v.map(Money::from_cents),
        recent_prices: vec![Money::from_cents(last)],
    });
    MarketView { stocks }
}

#[test]
fn view_and_intent_serde_roundtrip() {
    let mv = one_stock_view(1000, None);
    let j = serde_json::to_value(&mv).unwrap();
    let back: MarketView = serde_json::from_value(j).unwrap();
    assert_eq!(back.stocks.len(), 1);
}

use engine::strategy::{Intent, SelfView, Strategy, StrategyError, ZiNoiseStrategy};
use engine::orderbook::Side;

#[test]
fn zi_noise_arrival_rate_zero_produces_nothing() {
    let mut s = ZiNoiseStrategy::new(0.0, 100, 0.0, 1).unwrap();
    let mv = one_stock_view(1000, None);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.is_empty()); // arrival_rate=0 → 不动作
}

#[test]
fn zi_noise_arrival_rate_one_acts_on_some_stock() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 0.0, 1).unwrap();
    let mv = one_stock_view(1000, None);
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.3)); // 0.3<0.5 → 买
    assert_eq!(ints.len(), 1);
    assert!(matches!(
        ints[0],
        Intent::PlaceLimit {
            side: Side::Buy,
            qty: 100,
            ..
        }
    ));
    // 选中股票在 market 内
    if let Intent::PlaceLimit { code, .. } = &ints[0] {
        assert!(mv.stocks.contains_key(code));
    }
}

#[test]
fn zi_noise_chase_trend_buys_on_uptrend() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 1.0, 1).unwrap(); // chase_prob=1
    let mv = {
        let mut stocks = BTreeMap::new();
        stocks.insert(
            StockCode("600101".to_string()),
            StockView {
                best_bid: Some(Money::from_cents(999)),
                best_ask: Some(Money::from_cents(1001)),
                last_price: Money::from_cents(1050),
                fundamental_value: None,
                recent_prices: vec![
                    Money::from_cents(1000),
                    Money::from_cents(1020),
                    Money::from_cents(1050),
                ], // 上升
            },
        );
        MarketView { stocks }
    };
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints
        .iter()
        .any(|i| matches!(i, Intent::PlaceLimit { side: Side::Buy, .. })));
}

use engine::strategy::{PositionView, TargetPolicy, ValueStrategy};

#[test]
fn value_buys_when_undervalued() {
    // V=1000, target=TrackV{bias:0}→target=1000, margin=0.05→买阈 950。last=900<950 → 买
    let mut s = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, 0.05, 100).unwrap();
    let mv = one_stock_view(900, Some(1000)); // last=900, V=1000
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints
        .iter()
        .any(|i| matches!(i, Intent::PlaceLimit { side: Side::Buy, .. })));
}

#[test]
fn value_no_action_when_in_band() {
    let mut s = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, 0.05, 100).unwrap();
    let mv = one_stock_view(1000, Some(1000)); // last=1000 在 [950,1050] 带内
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    assert!(s
        .decide(&mv, &own, &mut SeqRng::new_f64(0.5))
        .is_empty());
}

#[test]
fn value_sells_when_overvalued_and_has_position() {
    let mut s = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, 0.05, 100).unwrap();
    let mv = one_stock_view(1100, Some(1000)); // last=1100>1050 → 卖
    let mut pos = BTreeMap::new();
    pos.insert(
        StockCode("600101".to_string()),
        PositionView {
            qty: 100,
            sellable_qty: 100,
            cost_price: Some(Money::from_cents(1000)),
        },
    );
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: pos,
    };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints
        .iter()
        .any(|i| matches!(i, Intent::PlaceLimit { side: Side::Sell, .. })));
}

#[test]
fn value_no_sell_without_position() {
    let mut s = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, 0.05, 100).unwrap();
    let mv = one_stock_view(1100, Some(1000));
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    }; // 无持仓
    assert!(s
        .decide(&mv, &own, &mut SeqRng::new_f64(0.5))
        .is_empty());
}

#[test]
fn value_target_policies_differ() {
    // Fixed(800) vs TrackV{bias:0.1} on V=1000 → 800 vs 1100
    let mut s_fixed =
        ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(800)), 0.01, 100).unwrap();
    let mut s_track = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.1 }, 0.01, 100).unwrap();
    let mv = one_stock_view(900, Some(1000)); // V=1000
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    // Fixed target=800, band [792,808]; last=900 > 808 → 应卖但无持仓 → 无动作
    assert!(s_fixed
        .decide(&mv, &own, &mut SeqRng::new_f64(0.5))
        .is_empty());
    // TrackV target=1100, band [1089,1111]; last=900 < 1089 → 买
    let ints = s_track.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints
        .iter()
        .any(|i| matches!(i, Intent::PlaceLimit { side: Side::Buy, .. })));
}

#[test]
fn value_ignores_stocks_without_visible_v() {
    let mut s = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, 0.05, 100).unwrap();
    let mv = one_stock_view(900, None); // V 不可见
    let own = SelfView {
        cash: Money::from_cents(1_000_000),
        positions: BTreeMap::new(),
    };
    assert!(s
        .decide(&mv, &own, &mut SeqRng::new_f64(0.5))
        .is_empty()); // 无 V 不动作
}

#[test]
fn value_rejects_invalid_params() {
    assert!(ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1000)), -0.1, 100).is_err());
    // margin<0
    assert!(ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1000)), 0.05, 0).is_err());
    // order_size=0
}

#[test]
fn zi_noise_rejects_invalid_params() {
    assert!(ZiNoiseStrategy::new(1.5, 100, 0.0, 1).is_err()); // arrival_rate>1
    assert!(ZiNoiseStrategy::new(0.5, 0, 0.0, 1).is_err()); // order_size_mean=0
    assert!(ZiNoiseStrategy::new(0.5, 100, 0.0, 0).is_err()); // tick_cents=0
    // 顺便确认合法参数 + StrategyError 变体可达（避免 use 未被检查）。
    let ok = ZiNoiseStrategy::new(0.5, 100, 0.1, 1);
    assert!(ok.is_ok());
    assert!(matches!(
        ZiNoiseStrategy::new(1.5, 100, 0.0, 1).err(),
        Some(StrategyError::InvalidParam { .. })
    ));
}
