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
