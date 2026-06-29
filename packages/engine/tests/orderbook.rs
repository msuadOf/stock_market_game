//! engine orderbook 模块集成测试（TDD 红绿循环）。
//!
//! Task 1：仅校验 OrderError 变体 to_string 携带字段、Side 相等/不等、OrderId 基础。
//! Task 2：Order/Trade 的 serde 往返保真 + MatchResult 空构造。
//! 撮合/OrderBook 行为在后续 Task 逐步补齐。
use engine::orderbook::{OrderError, OrderId, Side};

#[test]
fn order_error_and_side_basics() {
    // InvalidTick 的 to_string 应包含 tick 字段（铁律二：错误携带上下文）。
    let e1 = OrderError::InvalidTick {
        tick: engine::Money::ZERO,
    };
    assert!(e1.to_string().contains("tick"));

    // OrderNotFound 的 to_string 应包含被查订单的 id 数值。
    // OrderNotFound(OrderId) 为元组变体（与 DuplicateOrderId 一致，见 orderbook.rs 签名）。
    let e2 = OrderError::OrderNotFound(OrderId(7));
    assert!(e2.to_string().contains('7'));

    // Side 可判等（同向相等、异向不等）。
    assert_eq!(Side::Buy, Side::Buy);
    assert_ne!(Side::Buy, Side::Sell);
}

// ===== Task 2: Order/Trade/MatchResult 数据结构 + serde 往返 =====

use engine::orderbook::{AccountId, MatchResult, Order, Trade};
use engine::Money;

#[test]
fn order_trade_serde_roundtrip() {
    // Order 经 serde 序列化→反序列化后字段全等（保真）。
    let o = Order {
        id: OrderId(1),
        side: Side::Buy,
        price: Money::from_cents(1000),
        qty: 100,
        owner: AccountId(42),
        seq: 5,
    };
    let j = serde_json::to_value(&o).unwrap();
    let back: Order = serde_json::from_value(j).unwrap();
    assert_eq!(back.price.cents(), 1000);
    assert_eq!(back.qty, 100);

    // Trade 同理；Trade 派生 Eq+PartialEq，可直接整体比较。
    let t = Trade {
        price: Money::from_cents(1000),
        qty: 50,
        maker: AccountId(1),
        taker: AccountId(2),
    };
    let jt = serde_json::to_value(&t).unwrap();
    let bt: Trade = serde_json::from_value(jt).unwrap();
    assert_eq!(bt, t);
}

#[test]
fn match_result_default_empty() {
    // MatchResult 可构造为空（无成交、无残留挂单）。
    let r = MatchResult {
        trades: vec![],
        resting: None,
    };
    assert!(r.trades.is_empty());
    assert!(r.resting.is_none());
}

// ===== Task 3: OrderBook 结构 + new(tick) 构造校验 =====

use engine::orderbook::OrderBook;

#[test]
fn orderbook_new_validates_tick() {
    // 合法 tick(>0)：构造成功，空簿的最优买/卖价均为 None。
    let book = OrderBook::new(Money::from_cents(1)).unwrap();
    assert!(book.best_bid().is_none() && book.best_ask().is_none());

    // 非法 tick(<=0)：构造失败，返回 InvalidTick（铁律二：绝不静默吞错）。
    let err = OrderBook::new(Money::ZERO).unwrap_err();
    assert!(matches!(err, OrderError::InvalidTick { .. }));
}

// ===== Task 4: place —— 无对手盘挂单 + 价格/数量/tick 校验 =====

/// 测试辅助：构造一个 tick=1 分的空订单簿。
fn mk_book() -> OrderBook {
    OrderBook::new(Money::from_cents(1)).expect("tick=1 分恒合法")
}

#[test]
fn place_resting_order_with_no_counterparty() {
    // 无对手盘时买单应直接挂入簿：无成交、有残留、best_bid 反映该价。
    let mut book = mk_book();
    let o = Order {
        id: OrderId(1),
        side: Side::Buy,
        price: Money::from_cents(1000),
        qty: 100,
        owner: AccountId(1),
        seq: 0,
    };
    let r = book.place(o).unwrap();
    assert!(r.trades.is_empty()); // 无对手盘 → 无成交
    assert!(r.resting.is_some()); // 挂入簿
    assert_eq!(book.best_bid(), Some(Money::from_cents(1000)));
}

#[test]
fn place_rejects_invalid_price_and_qty() {
    let mut book = mk_book();

    // 非 tick 整数倍：tick=1 分时所有整数价格都整除，故改用 tick=5 分、价格 1003 分
    // (1003 % 5 = 3 != 0) 触发非整除分支 → InvalidPrice。
    let mut book2 = OrderBook::new(Money::from_cents(5)).expect("tick=5 分恒合法");
    let bad = Order {
        id: OrderId(1),
        side: Side::Buy,
        price: Money::from_cents(1003), // 1003 % 5 != 0
        qty: 100,
        owner: AccountId(1),
        seq: 0,
    };
    assert!(matches!(
        book2.place(bad).unwrap_err(),
        OrderError::InvalidPrice { .. }
    ));

    // qty == 0 → InvalidQty(0)。
    let zero = Order {
        id: OrderId(2),
        side: Side::Buy,
        price: Money::from_cents(1000),
        qty: 0,
        owner: AccountId(1),
        seq: 0,
    };
    assert!(matches!(
        book.place(zero).unwrap_err(),
        OrderError::InvalidQty(0)
    ));
}
