//! engine account + strategy-trait 模块集成测试（TDD 红绿循环）。
// `Strategy` 当前在 Task 1 骨架测试中尚未直接使用，但后续 account 任务（持有 `Box<dyn Strategy>`、
// 注入 NPC 策略）会消费它——此处提前 import 以保持 plan 预期的导入集合，待消费后移除 allow。
#[allow(unused_imports)]
use engine::strategy::{Intent, MarketView, Strategy};
use engine::orderbook::Side;
use engine::Money;

#[test]
fn intent_and_marketview_construct() {
    let i = Intent::Pass;
    let i2 = Intent::PlaceLimit { side: Side::Buy, price: Money::from_cents(1000), qty: 100 };
    assert!(matches!(i, Intent::Pass));
    assert!(matches!(i2, Intent::PlaceLimit { side: Side::Buy, qty: 100, .. }));

    let mv = MarketView {
        best_bid: Some(Money::from_cents(999)),
        best_ask: Some(Money::from_cents(1001)),
        last_price: Money::from_cents(1000),
    };
    assert_eq!(mv.last_price.cents(), 1000);
}

use engine::account::{AccountError, AccountKind, StockCode};

#[test]
fn account_error_and_kind_basics() {
    let e1 = AccountError::InsufficientCash {
        needed: Money::from_cents(1500),
        have: Money::from_cents(1000),
    };
    assert!(e1.to_string().contains("cash"));
    let e2 = AccountError::NoPosition(StockCode("600101".to_string()));
    assert!(e2.to_string().contains("600101"));
    assert_ne!(AccountKind::Retail, AccountKind::Player);
}

use engine::account::Position;

#[test]
fn position_cost_price_integer_rounding() {
    // 整除：invested=100000(1000元), recovered=0, qty=100 → cost=1000 分/股 = 10.00 元
    let p = Position {
        qty: 100,
        t1_locked: 0,
        invested_cents: 100_000,
        recovered_cents: 0,
    };
    assert_eq!(p.cost_price().unwrap().cents(), 1000);

    // 加权：invested=220000, qty=200 → 1100 分 = 11.00
    let p2 = Position {
        qty: 200,
        t1_locked: 0,
        invested_cents: 220_000,
        recovered_cents: 0,
    };
    assert_eq!(p2.cost_price().unwrap().cents(), 1100);

    // 非整除 half-to-even：invested=1000, recovered=0, qty=3 → 1000/3=333.33… → 333（<.5 向下）
    let p3 = Position {
        qty: 3,
        t1_locked: 0,
        invested_cents: 1000,
        recovered_cents: 0,
    };
    assert_eq!(p3.cost_price().unwrap().cents(), 333);

    // 恰好半（half-to-even）：invested=5, qty=2 → 2.5 → 偶数取 2
    let p4 = Position {
        qty: 2,
        t1_locked: 0,
        invested_cents: 5,
        recovered_cents: 0,
    };
    assert_eq!(p4.cost_price().unwrap().cents(), 2); // 2.5 → 2 (偶)
    // invested=7, qty=2 → 3.5 → 偶数取 4
    let p5 = Position {
        qty: 2,
        t1_locked: 0,
        invested_cents: 7,
        recovered_cents: 0,
    };
    assert_eq!(p5.cost_price().unwrap().cents(), 4); // 3.5 → 4 (偶)
}

#[test]
fn position_cost_price_negative() {
    // 净投入/持仓：invested < recovered → 负成本
    // invested=100000, recovered=200000, qty=100 → (100000-200000)/100 = -1000 分
    let p = Position {
        qty: 100,
        t1_locked: 0,
        invested_cents: 100_000,
        recovered_cents: 200_000,
    };
    assert_eq!(p.cost_price().unwrap().cents(), -1000);
}

#[test]
fn position_cost_price_none_when_zero_qty() {
    let p = Position {
        qty: 0,
        t1_locked: 0,
        invested_cents: 0,
        recovered_cents: 0,
    };
    assert!(p.cost_price().is_none());
    assert_eq!(p.sellable(), 0);
}

#[test]
fn position_sellable_minus_t1_locked() {
    let p = Position {
        qty: 100,
        t1_locked: 30,
        invested_cents: 0,
        recovered_cents: 0,
    };
    assert_eq!(p.sellable(), 70);
}
