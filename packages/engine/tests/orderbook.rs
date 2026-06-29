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

// ===== Task 5: place 撮合核心 —— 交叉即成交、部分成交、price-time、成交价=maker价 =====

/// 测试辅助：构造一笔卖单（价格以「分」给）。seq=0 由 place 在挂入时重分配。
fn sell(id: u64, price_cents: i64, qty: u32, owner: u64) -> Order {
    Order {
        id: OrderId(id),
        side: Side::Sell,
        price: Money::from_cents(price_cents),
        qty,
        owner: AccountId(owner),
        seq: 0,
    }
}

/// 测试辅助：构造一笔买单（价格以「分」给）。seq=0 由 place 在挂入时重分配。
fn buy(id: u64, price_cents: i64, qty: u32, owner: u64) -> Order {
    Order {
        id: OrderId(id),
        side: Side::Buy,
        price: Money::from_cents(price_cents),
        qty,
        owner: AccountId(owner),
        seq: 0,
    }
}

#[test]
fn match_one_to_one_exact_cross() {
    // 卖 10.00×100（owner=10，先挂为 maker）vs 买 10.00×100（owner=20，新进为 taker）。
    // 买价 == 卖价 → 交叉成交：1 笔 trade，价 1000、量 100，maker=卖方、taker=买方，双方清空。
    let mut book = mk_book();
    book.place(sell(1, 1000, 100, 10)).unwrap();
    let r = book.place(buy(2, 1000, 100, 20)).unwrap();
    assert_eq!(r.trades.len(), 1);
    let t = &r.trades[0];
    assert_eq!(t.price.cents(), 1000); // maker 价
    assert_eq!(t.qty, 100);
    assert_eq!(t.maker, AccountId(10)); // 被动方 = 卖方(先挂)
    assert_eq!(t.taker, AccountId(20)); // 主动方 = 买方(新进)
    assert!(r.resting.is_none()); // 双方都清空
    assert!(book.best_ask().is_none() && book.best_bid().is_none());
}

#[test]
fn match_partial_fill_leaves_resting() {
    // 卖 250 股 vs 买 100 股：成交 100，买单全成交(resting=None)，卖单余 150 留簿；
    // 再买 150 应清空卖盘。
    let mut book = mk_book();
    book.place(sell(1, 1000, 250, 10)).unwrap();
    let r = book.place(buy(2, 1000, 100, 20)).unwrap();
    assert_eq!(r.trades.len(), 1);
    assert_eq!(r.trades[0].qty, 100);
    assert!(r.resting.is_none()); // 买单全成交

    // 卖单剩余 150 仍在簿 → 再买 150 清空。
    let r2 = book.place(buy(3, 1000, 150, 30)).unwrap();
    assert_eq!(r2.trades.len(), 1);
    assert_eq!(r2.trades[0].qty, 150);
    assert!(book.best_ask().is_none()); // 卖盘清空
}

#[test]
fn match_price_priority_better_counter_first() {
    // 卖盘 ask 10.01×100 + ask 10.00×100(更优) vs 买 10.01×150：
    // 应先吃更优的 10.00(价 1000、量 100)，再吃 10.01(价 1001、量 50)；买单 150 全成交。
    let mut book = mk_book();
    book.place(sell(1, 1001, 100, 10)).unwrap(); // ask 10.01
    book.place(sell(2, 1000, 100, 11)).unwrap(); // ask 10.00 (更优)
    let r = book.place(buy(3, 1001, 150, 20)).unwrap(); // 买 10.01×150
    assert_eq!(r.trades.len(), 2);
    assert_eq!(r.trades[0].price.cents(), 1000); // 先吃更优的 10.00
    assert_eq!(r.trades[0].qty, 100);
    assert_eq!(r.trades[1].price.cents(), 1001); // 再吃 10.01
    assert_eq!(r.trades[1].qty, 50); // 买单剩 50
    assert!(r.resting.is_none()); // 买 150 全成交
}

#[test]
fn match_time_priority_same_price_fifo() {
    // 两卖同价 10.00：A(owner=10)先挂、B(owner=11)后挂 → 买 100 应先吃 A（同价 FIFO）。
    let mut book = mk_book();
    book.place(sell(1, 1000, 100, 10)).unwrap(); // 先挂 A owner=10
    book.place(sell(2, 1000, 100, 11)).unwrap(); // 后挂 B owner=11
    let r = book.place(buy(3, 1000, 100, 20)).unwrap(); // 吃 100 股
    assert_eq!(r.trades.len(), 1);
    assert_eq!(r.trades[0].maker, AccountId(10)); // 先挂的 A 先成交
}

#[test]
fn match_fill_price_is_passive_side() {
    // 卖 10.00 vs 买 10.05（愿付更高）：成交价应取 maker(卖方)价 1000，而非 1005。
    let mut book = mk_book();
    book.place(sell(1, 1000, 100, 10)).unwrap(); // 卖 10.00
    let r = book.place(buy(2, 1005, 100, 20)).unwrap(); // 买 10.05（愿付更高）
    assert_eq!(r.trades[0].price.cents(), 1000); // 成交价 = maker(卖方)价,非 10.05
}

// ===== Task 6: cancel 撤单 =====

#[test]
fn cancel_removes_and_returns_order() {
    // 挂买单后撤单：cancel 应返回该单（id/qty 正确），best_bid 变 None；
    // 再次撤同 id → OrderNotFound（已不在簿）。
    let mut book = mk_book();
    book.place(buy(1, 1000, 100, 10)).unwrap();
    assert_eq!(book.best_bid(), Some(Money::from_cents(1000)));

    let removed = book.cancel(OrderId(1)).unwrap();
    assert_eq!(removed.id, OrderId(1));
    assert_eq!(removed.qty, 100);
    assert!(book.best_bid().is_none()); // 撤后空簿

    // 再次撤同 id → OrderNotFound(OrderId)，元组变体；用 matches! + 模式断言变体与携带的 id。
    // （OrderError 未 derive PartialEq，故不能用 assert_eq!；matches! 不要求 PartialEq。）
    let err = book.cancel(OrderId(1)).unwrap_err();
    assert!(matches!(err, OrderError::OrderNotFound(OrderId(1))));
}

#[test]
fn cancel_unknown_id_errors() {
    // 撤一个从未存在的 id → OrderNotFound（铁律二：绝不静默返回空/默认）。
    // 元组变体 OrderNotFound(OrderId)；断言变体正确且携带请求的 id=999。
    let mut book = mk_book();
    assert!(matches!(
        book.cancel(OrderId(999)).unwrap_err(),
        OrderError::OrderNotFound(OrderId(999))
    ));
}
