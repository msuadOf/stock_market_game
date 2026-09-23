//! engine market 模块集成测试（TDD 红绿循环）。
//! 任务 26 修复轮：从 ccf0490 恢复；删除 5 个 `evolve_v_*` 测试与 VParams 专属断言，
//! 适配 4 参 `Market::new`；**全部非 V 涨跌停/价格笼子/撮合语义测试逐字保留**。
//! 体积超过 250 行约定，按 task-6 目录先例拆 `price_limits.rs`（`--test market` 不变）。
use engine::account::StockCode;
use engine::market::{Market, MarketError};
use engine::Money;

mod price_limits;

#[test]
fn market_error_basics() {
    let e = MarketError::LimitExceeded {
        code: StockCode("600101".to_string()),
        price: Money::from_cents(1101),
        down: Money::from_cents(900),
        up: Money::from_cents(1100),
    };
    assert!(e.to_string().contains("600101"));
    let e2 = MarketError::InvalidParams {
        reason: "bad".to_string(),
    };
    assert!(e2.to_string().contains("bad"));
}

fn mk_market() -> Market {
    // last_close=last_price=10.00，limit=0.10，tick=0.01
    Market::new(
        StockCode("600101".to_string()),
        Money::from_cents(1000),
        0.10,
        Money::from_cents(1),
    )
    .unwrap()
}

#[test]
fn market_new_and_limit_stops() {
    let m = mk_market();
    assert_eq!(m.last_price().cents(), 1000);
    assert_eq!(m.last_close().cents(), 1000);
    assert_eq!(m.up_stop().unwrap().cents(), 1100); // 1000 × 1.10
    assert_eq!(m.down_stop().unwrap().cents(), 900); // 1000 × 0.90
}

#[test]
fn market_new_rejects_invalid() {
    // limit_pct ∉ (0,1)
    assert!(Market::new(
        StockCode("x".to_string()),
        Money::from_cents(1000),
        1.5,
        Money::from_cents(1)
    )
    .is_err());
    assert!(Market::new(
        StockCode("x".to_string()),
        Money::from_cents(1000),
        0.0,
        Money::from_cents(1)
    )
    .is_err());
    // initial_price ≤ 0
    assert!(Market::new(
        StockCode("x".to_string()),
        Money::ZERO,
        0.10,
        Money::from_cents(1)
    )
    .is_err());
}

use engine::orderbook::{AccountId, Order, OrderId, Side};

fn buy(id: u64, price_cents: i64, qty: u32) -> Order {
    Order {
        id: OrderId(id),
        side: Side::Buy,
        price: Money::from_cents(price_cents),
        qty,
        original_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner: AccountId(1),
        seq: 0,
    }
}
fn sell(id: u64, price_cents: i64, qty: u32) -> Order {
    Order {
        id: OrderId(id),
        side: Side::Sell,
        price: Money::from_cents(price_cents),
        qty,
        original_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner: AccountId(2),
        seq: 0,
    }
}

#[test]
fn place_updates_last_price_on_trade() {
    let mut m = mk_market(); // last_price=1000
    m.place(sell(1, 1000, 100)).unwrap();
    assert_eq!(m.last_price().cents(), 1000); // 无成交，last_price 不变
    m.place(buy(2, 1000, 100)).unwrap(); // 撮合成交价 1000
    assert_eq!(m.last_price().cents(), 1000); // 末笔 trade 价 1000
}

#[test]
fn place_match_result_matches_book() {
    let mut m = mk_market();
    m.place(sell(1, 1000, 100)).unwrap();
    let r = m.place(buy(2, 1000, 100)).unwrap();
    assert_eq!(r.trades.len(), 1);
    assert_eq!(r.trades[0].price.cents(), 1000);
    assert_eq!(r.trades[0].qty, 100);
}

#[test]
fn end_of_day_resets_last_close() {
    let mut m = mk_market(); // last_close=last_price=1000, up_stop=1100
                             // 成交一笔 1050（在涨跌停内）：先挂卖 1050，再买 1050 吃掉
    m.place(sell(1, 1050, 100)).unwrap();
    m.place(buy(2, 1050, 100)).unwrap();
    assert_eq!(m.last_price().cents(), 1050);
    m.end_of_day();
    assert_eq!(m.last_close().cents(), 1050); // 昨收更新为 last_price
    assert_eq!(m.up_stop().unwrap().cents(), 1155); // 1050×1.10=1155，基准已更新
}

#[test]
fn depth_passes_through_book() {
    let mut m = mk_market();
    m.place(sell(1, 1000, 100)).unwrap();
    m.place(sell(2, 1000, 50)).unwrap();
    let d = m.ask_depth();
    assert_eq!(d[0], (Money::from_cents(1000), 150));
}

#[test]
fn reexport_from_crate_root() {
    use engine::{Market, MarketError};
    let _ = Market::new(
        StockCode("x".to_string()),
        Money::from_cents(1000),
        0.10,
        Money::from_cents(1),
    )
    .unwrap();
    let _: MarketError = MarketError::InvalidParams {
        reason: "x".to_string(),
    };
}
