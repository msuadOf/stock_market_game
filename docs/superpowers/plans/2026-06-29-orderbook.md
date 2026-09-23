# engine `orderbook` 模块 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: 用 ultracode workflow 串行 TDD（红→绿→提交），与 money/config 同款，一路跑到全绿 + 独立验证门。Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 engine 的订单簿撮合模块 —— 价格-时间优先的限价撮合引擎（ADR-0005 §3 撮合驱动价格的核心），只依赖 `Money`。

**Architecture:** `OrderBook` 用两个 `BTreeMap` 维护买卖盘（价格+seq 排序实现 price-time 优先）。`place(order)` 与对手盘逐档撮合产出 `Trade` 列表 + 残留挂单；`cancel(id)` 撤单；`best_bid/best_ask/depth` 盘口只读。价格全程 `Money`（分），tick 可配。纯逻辑、与 account/market/strategy 解耦。

**Tech Stack:** Rust 2021，`std::collections::BTreeMap`（+ `Reverse` for 买盘降序），`serde`/`serde_json`/`thiserror`（workspace 已就位）。测试 `cargo test -p engine`。

## Global Constraints

- **铁律二（防御式）：** 非法价格/数量/tick → `Result` + 显式 `OrderError`；**禁止**静默截断价格/数量、禁止负剩余量、禁止 `unwrap` 业务路径、禁止吞错。
- **铁律一（TDD）：** 每任务严格 红→绿→提交；先写失败测试再写实现。
- 价格全程 `Money`（i64 分）；**不得**用 `f64` 存储价格。tick 校验用整数 cents 取模。
- 依赖只用 workspace 已声明的 `serde`/`serde_json`/`thiserror` + std；**不引入新依赖**。
- 复用 `engine::money::{Money, MoneyError}`（`Money::from_cents`/`cents`/`ZERO`，Money 已 derive Ord 可作 BTreeMap key）。
- 提交信息 Conventional Commits，scope `engine`。
- 命名/注释风格匹配 `packages/engine/src/money.rs`、`config.rs`（中文 doc 注释、thiserror `#[error]`、`ok_or_else` 模式）。

## File Structure

```
packages/engine/src/
├── lib.rs          # 修改：追加 `pub mod orderbook;` + re-export
└── orderbook.rs    # 新建：Side/Order/Trade/OrderId/OrderError/OrderBook + 撮合
packages/engine/tests/
└── orderbook.rs    # 新建：集成测试（TDD 红绿驱动）
```

`OrderId` 本模块自带 newtype（`pub struct OrderId(pub u64)`），不依赖未来 account 模块。

---

## Task 1: `OrderError` + 基础类型（Side/OrderId）+ 模块骨架

**Files:**
- Create: `packages/engine/src/orderbook.rs`
- Modify: `packages/engine/src/lib.rs`
- Test: `packages/engine/tests/orderbook.rs`

**Interfaces:**
- Produces: `pub enum OrderError { InvalidPrice, InvalidQty, DuplicateOrderId, OrderNotFound, InvalidTick }`（thiserror，每变体带上下文字段）；`pub enum Side { Buy, Sell }`；`pub struct OrderId(pub u64)`。

- [ ] **Step 1: 写失败测试**

`packages/engine/tests/orderbook.rs`：
```rust
//! engine orderbook 模块集成测试（TDD 红绿循环）。
use engine::orderbook::{OrderError, Side, OrderId};

#[test]
fn order_error_and_side_basics() {
    let e1 = OrderError::InvalidTick { tick: engine::Money::ZERO };
    assert!(e1.to_string().contains("tick"));
    let e2 = OrderError::OrderNotFound { id: OrderId(7) };
    assert!(e2.to_string().contains("7"));
    assert_eq!(Side::Buy, Side::Buy);
    assert_ne!(Side::Buy, Side::Sell);
}
```

`packages/engine/src/lib.rs` 末尾追加：
```rust
pub mod orderbook;
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `unresolved module orderbook` / `cannot find OrderError`（RED）。

- [ ] **Step 3: 写最小实现**

`packages/engine/src/orderbook.rs`：
```rust
//! 订单簿撮合引擎：价格-时间优先的限价撮合。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-orderbook-design.md。
//! ADR-0005 §3 撮合驱动价格的核心；纯逻辑，只依赖 Money，与 account/market/strategy 解耦。

use crate::money::Money;
use thiserror::Error;

/// 买卖方向。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Side {
    Buy,
    Sell,
}

/// 订单 id（单调自增）。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub struct OrderId(pub u64);

/// orderbook 操作失败。绝不静默吞掉（铁律二）。
#[derive(Debug, Error)]
pub enum OrderError {
    /// 价格非法：为负 / 非 tick 整数倍 / 超范围。
    #[error("invalid price {price:?}: {reason} (tick {tick:?})")]
    InvalidPrice { price: Money, tick: Money, reason: String },
    /// 数量非法：qty == 0。
    #[error("invalid qty: {0} (must be > 0)")]
    InvalidQty(u32),
    /// 重复订单 id（防御式，自增不应触发）。
    #[error("duplicate order id: {0:?}")]
    DuplicateOrderId(OrderId),
    /// 撤单时 id 不存在。
    #[error("order not found: {0:?}")]
    OrderNotFound(OrderId),
    /// 构造时 tick 非法：<= 0。
    #[error("invalid tick: {tick:?} (must be > 0)")]
    InvalidTick { tick: Money },
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（1 test passed）。注意 lib.rs 暂只 `pub mod orderbook`，不 re-export OrderBook（尚未定义）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/orderbook.rs packages/engine/src/lib.rs packages/engine/tests/orderbook.rs
git commit -m "test(engine): 建立 orderbook 模块骨架 + OrderError/Side/OrderId"
```
（末尾附 `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`，下同。）

---

## Task 2: `Order` + `Trade` + `MatchResult` 数据结构 + serde 往返

**Files:**
- Modify: `packages/engine/src/orderbook.rs`
- Modify: `packages/engine/tests/orderbook.rs`

**Interfaces:**
- Produces: `pub struct Order { id: OrderId, side: Side, price: Money, qty: u32, owner: AccountId, seq: u64 }`；`pub struct Trade { price: Money, qty: u32, maker: AccountId, taker: AccountId }`；`pub struct MatchResult { trades: Vec<Trade>, resting: Option<Order> }`。
- `AccountId`：本模块用轻量 newtype 占位（`pub struct AccountId(pub u64)`），不引入对 account 的依赖。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/orderbook.rs`：
```rust
use engine::orderbook::{Order, Trade, MatchResult, AccountId, Side, OrderId};
use engine::Money;

#[test]
fn order_trade_serde_roundtrip() {
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

    let t = Trade { price: Money::from_cents(1000), qty: 50, maker: AccountId(1), taker: AccountId(2) };
    let jt = serde_json::to_value(&t).unwrap();
    let bt: Trade = serde_json::from_value(jt).unwrap();
    assert_eq!(bt, t);
}

#[test]
fn match_result_default_empty() {
    let r = MatchResult { trades: vec![], resting: None };
    assert!(r.trades.is_empty());
    assert!(r.resting.is_none());
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `cannot find type Order`（RED）。

- [ ] **Step 3: 写实现**

追加到 `packages/engine/src/orderbook.rs`：
```rust
/// 账户 id 占位 newtype（待 account 模块统一；本模块不依赖 account）。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize)]
pub struct AccountId(pub u64);

/// 单笔限价挂单（撮合发生时为不可变快照；簿内以 OrderId/seq 引用）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub side: Side,
    pub price: Money,
    pub qty: u32,
    pub owner: AccountId,
    /// 时间序：同价位排序键（先挂先成交）。
    pub seq: u64,
}

/// 一笔成交。成交价取被动方（maker）的价格。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct Trade {
    pub price: Money,
    pub qty: u32,
    /// 被动方（较早挂单、提供流动性）。
    pub maker: AccountId,
    /// 主动方（新进入、吃流动性）。
    pub taker: AccountId,
}

/// 撮合一笔新单的结果。
pub struct MatchResult {
    /// 本次撮合产生的成交（按发生顺序）。
    pub trades: Vec<Trade>,
    /// 新单若有剩余量，挂入簿的残留订单；None 表示全成交。
    pub resting: Option<Order>,
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/orderbook.rs packages/engine/tests/orderbook.rs
git commit -m "feat(engine): orderbook Order/Trade/MatchResult 数据结构 + serde"
```

---

## Task 3: `OrderBook` 结构 + `new(tick)` 构造校验

**Files:**
- Modify: `packages/engine/src/orderbook.rs`
- Modify: `packages/engine/tests/orderbook.rs`

**Interfaces:**
- Produces: `pub struct OrderBook { bids, asks, next_seq, next_id, tick }`；`OrderBook::new(tick: Money) -> Result<OrderBook, OrderError>`（tick ≤ 0 → Err）。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/orderbook.rs`：
```rust
use engine::orderbook::OrderBook;

#[test]
fn orderbook_new_validates_tick() {
    let book = OrderBook::new(Money::from_cents(1)).unwrap();
    assert!(book.best_bid().is_none() && book.best_ask().is_none()); // 空簿

    let err = OrderBook::new(Money::ZERO).unwrap_err();
    assert!(matches!(err, OrderError::InvalidTick { .. }));
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `cannot find type OrderBook` / `new`（RED）。

- [ ] **Step 3: 写实现**

追加到 `packages/engine/src/orderbook.rs`（顶部加 `use std::cmp::Reverse; use std::collections::BTreeMap;`）：
```rust
use std::cmp::Reverse;
use std::collections::BTreeMap;

/// 单只股票的订单簿。
///
/// 买盘按「价高优先、同价先挂优先」：key = `(Reverse(price), seq)`。
/// 卖盘按「价低优先、同价先挂优先」：key = `(price, seq)`。
pub struct OrderBook {
    bids: BTreeMap<(Reverse<Money>, u64), Order>,
    asks: BTreeMap<(Money, u64), Order>,
    next_seq: u64,
    next_id: u64,
    tick: Money,
}

impl OrderBook {
    /// 构造。tick 必须 > 0（价格最小变动）。
    pub fn new(tick: Money) -> Result<OrderBook, OrderError> {
        if tick.cents() <= 0 {
            return Err(OrderError::InvalidTick { tick });
        }
        Ok(OrderBook {
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            next_seq: 0,
            next_id: 0,
            tick,
        })
    }

    /// 买盘最优价（最高买价）。空簿返回 None。
    pub fn best_bid(&self) -> Option<Money> {
        self.bids.first_key_value().map(|((Reverse(p), _), _)| *p)
    }

    /// 卖盘最优价（最低卖价）。空簿返回 None。
    pub fn best_ask(&self) -> Option<Money> {
        self.asks.first_key_value().map(|((p, _), _)| *p)
    }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/orderbook.rs packages/engine/tests/orderbook.rs
git commit -m "feat(engine): OrderBook 结构 + new(tick) 校验 + best_bid/best_ask"
```

---

## Task 4: `place` —— 无对手盘挂单 + 价格/数量/tick 校验

**Files:**
- Modify: `packages/engine/src/orderbook.rs`
- Modify: `packages/engine/tests/orderbook.rs`

**Interfaces:**
- Produces: `OrderBook::place(&mut self, order: Order) -> Result<MatchResult, OrderError>`。本任务先实现「校验 + 无对手盘时挂入簿」（不撮合），为 Task 5 撮合铺路。校验：price.cents() >= 0、price % tick == 0、qty > 0。
- Consumes: `Order`（Task 2）、`MatchResult`（Task 2）。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/orderbook.rs`：
```rust
fn mk_book() -> OrderBook {
    OrderBook::new(Money::from_cents(1)).unwrap()
}

#[test]
fn place_resting_order_with_no_counterparty() {
    let mut book = mk_book();
    let o = Order {
        id: OrderId(1), side: Side::Buy, price: Money::from_cents(1000),
        qty: 100, owner: AccountId(1), seq: 0,
    };
    let r = book.place(o).unwrap();
    assert!(r.trades.is_empty());     // 无对手盘，无成交
    assert!(r.resting.is_some());     // 挂入簿
    assert_eq!(book.best_bid(), Some(Money::from_cents(1000)));
}

#[test]
fn place_rejects_invalid_price_and_qty() {
    let mut book = mk_book();
    // 非 tick 整数倍（tick=1分，价格 1005 分… 等等 tick=1 时所有整数都整除。
    //  改用 tick=1分，价格 1000+1=1001 也整除 → 用更小的 tick 触发非整除需 tick>1）。
    let mut book2 = OrderBook::new(Money::from_cents(5)).unwrap(); // tick=5分
    let bad = Order {
        id: OrderId(1), side: Side::Buy, price: Money::from_cents(1003), // 1003 % 5 != 0
        qty: 100, owner: AccountId(1), seq: 0,
    };
    assert!(matches!(book2.place(bad).unwrap_err(), OrderError::InvalidPrice { .. }));

    // qty == 0
    let zero = Order {
        id: OrderId(2), side: Side::Buy, price: Money::from_cents(1000),
        qty: 0, owner: AccountId(1), seq: 0,
    };
    assert!(matches!(book.place(zero).unwrap_err(), OrderError::InvalidQty(0)));
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `no method place`（RED）。

- [ ] **Step 3: 写实现**

在 `impl OrderBook` 内追加：
```rust
    /// 撮合新单。校验价格/数量后与对手盘逐档撮合；剩余挂入己方簿。
    pub fn place(&mut self, mut order: Order) -> Result<MatchResult, OrderError> {
        // 校验数量
        if order.qty == 0 {
            return Err(OrderError::InvalidQty(order.qty));
        }
        // 校验价格：非负 + tick 整数倍
        if order.price.cents() < 0 || order.price.cents() % self.tick.cents() != 0 {
            return Err(OrderError::InvalidPrice {
                price: order.price,
                tick: self.tick,
                reason: if order.price.cents() < 0 {
                    "negative".to_string()
                } else {
                    "not a multiple of tick".to_string()
                },
            });
        }

        // Task 5 在此插入撮合循环；当前先「无对手盘 → 直接挂入」。
        let seq = self.next_seq;
        self.next_seq += 1;
        order.seq = seq;
        self.insert_resting(order.clone());
        Ok(MatchResult { trades: vec![], resting: Some(order) })
    }

    /// 将残留订单挂入对应簿（内部辅助）。
    fn insert_resting(&mut self, order: Order) {
        match order.side {
            Side::Buy => {
                self.bids.insert((Reverse(order.price), order.seq), order);
            }
            Side::Sell => {
                self.asks.insert((order.price, order.seq), order);
            }
        }
    }
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/orderbook.rs packages/engine/tests/orderbook.rs
git commit -m "feat(engine): OrderBook::place 校验 + 无对手盘挂入簿(撮合铺路)"
```

---

## Task 5: `place` 撮合核心 —— 交叉即成交、部分成交、price-time、成交价=maker价

**Files:**
- Modify: `packages/engine/src/orderbook.rs`（替换 Task 4 的「无对手盘直接挂入」为完整撮合）
- Modify: `packages/engine/tests/orderbook.rs`

**Interfaces:**
- Produces: 完整 `place` 撮合：买价 ≥ 最优卖价（或卖价 ≤ 最优买价）时逐档成交；`fill_qty = min(new.qty, maker.qty)`；`fill_price = maker.price`；maker 清零移出簿；new 剩余挂入。
- Consumes: `Trade`、`Order`、`Side`、`MatchResult`。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/orderbook.rs`：
```rust
fn sell(id: u64, price_cents: i64, qty: u32, owner: u64) -> Order {
    Order { id: OrderId(id), side: Side::Sell, price: Money::from_cents(price_cents), qty, owner: AccountId(owner), seq: 0 }
}
fn buy(id: u64, price_cents: i64, qty: u32, owner: u64) -> Order {
    Order { id: OrderId(id), side: Side::Buy, price: Money::from_cents(price_cents), qty, owner: AccountId(owner), seq: 0 }
}

#[test]
fn match_one_to_one_exact_cross() {
    let mut book = mk_book();
    book.place(sell(1, 1000, 100, 10)).unwrap(); // 卖 10.00×100 owner=10
    let r = book.place(buy(2, 1000, 100, 20)).unwrap(); // 买 10.00×100 owner=20
    assert_eq!(r.trades.len(), 1);
    let t = &r.trades[0];
    assert_eq!(t.price.cents(), 1000); // maker 价
    assert_eq!(t.qty, 100);
    assert_eq!(t.maker, AccountId(10)); // 被动方=卖方(先挂)
    assert_eq!(t.taker, AccountId(20)); // 主动方=买方(新进)
    assert!(r.resting.is_none()); // 双方都清空
    assert!(book.best_ask().is_none() && book.best_bid().is_none());
}

#[test]
fn match_partial_fill_leaves_resting() {
    let mut book = mk_book();
    book.place(sell(1, 1000, 250, 10)).unwrap(); // 卖 250 股
    let r = book.place(buy(2, 1000, 100, 20)).unwrap(); // 买 100 股
    assert_eq!(r.trades.len(), 1);
    assert_eq!(r.trades[0].qty, 100);
    assert!(r.resting.is_none()); // 买单全成交
    // 卖单剩余 150 仍在簿
    let r2 = book.place(buy(3, 1000, 150, 30)).unwrap();
    assert_eq!(r2.trades.len(), 1);
    assert_eq!(r2.trades[0].qty, 150);
    assert!(book.best_ask().is_none()); // 卖盘清空
}

#[test]
fn match_price_priority_better_counter_first() {
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
    let mut book = mk_book();
    book.place(sell(1, 1000, 100, 10)).unwrap(); // 先挂 A owner=10
    book.place(sell(2, 1000, 100, 11)).unwrap(); // 后挂 B owner=11
    let r = book.place(buy(3, 1000, 100, 20)).unwrap(); // 吃 100 股
    assert_eq!(r.trades.len(), 1);
    assert_eq!(r.trades[0].maker, AccountId(10)); // 先挂的 A 先成交
}

#[test]
fn match_fill_price_is_passive_side() {
    let mut book = mk_book();
    book.place(sell(1, 1000, 100, 10)).unwrap(); // 卖 10.00
    let r = book.place(buy(2, 1005, 100, 20)).unwrap(); // 买 10.05（愿付更高）
    assert_eq!(r.trades[0].price.cents(), 1000); // 成交价=maker(卖方)价,非 10.05
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: Task 4 的「无对手盘直接挂入」实现会让这些测试失败（trades 空、resting 错）（RED）。

- [ ] **Step 3: 写实现**

用以下完整撮合逻辑**替换** Task 4 中 `place` 方法体里「Task 5 在此插入撮合循环；当前先无对手盘直接挂入」那段（即替换 `place` 末尾的 4 行占位）：
```rust
        let mut trades: Vec<Trade> = Vec::new();
        let taker = order.owner;
        let side = order.side;

        // 与对手盘逐档撮合，直到无交叉或 new.qty 耗尽。
        loop {
            if order.qty == 0 {
                break;
            }
            // 是否交叉？买单：买价 >= 最优卖价；卖单：卖价 <= 最优买价。
            let crossed = match side {
                Side::Buy => self
                    .asks
                    .first_key_value()
                    .map(|((ask_p, _), _)| order.price >= *ask_p)
                    .unwrap_or(false),
                Side::Sell => self
                    .bids
                    .first_key_value()
                    .map(|((Reverse(bid_p), _), _)| order.price >= *bid_p)
                    .unwrap_or(false),
            };
            if !crossed {
                break;
            }

            // 取对手最优档（-maker-），成交价取 maker.price。
            let (maker, fill_price) = match side {
                Side::Buy => {
                    let entry = self.asks.first_entry().expect("crossed => non-empty");
                    let m = entry.get().clone();
                    let p = m.price;
                    (m, p)
                }
                Side::Sell => {
                    let entry = self.bids.first_entry().expect("crossed => non-empty");
                    let m = entry.get().clone();
                    let p = m.price;
                    (m, p)
                }
            };

            let fill_qty = order.qty.min(maker.qty);
            order.qty -= fill_qty;
            // 更新对手档（扣减或移除）
            match side {
                Side::Buy => {
                    if maker.qty == fill_qty {
                        self.asks.pop_first();
                    } else {
                        let key = (maker.price, maker.seq);
                        if let Some(m) = self.asks.get_mut(&key) {
                            m.qty -= fill_qty;
                        }
                    }
                }
                Side::Sell => {
                    if maker.qty == fill_qty {
                        self.bids.pop_first();
                    } else {
                        let key = (Reverse(maker.price), maker.seq);
                        if let Some(m) = self.bids.get_mut(&key) {
                            m.qty -= fill_qty;
                        }
                    }
                }
            }

            trades.push(Trade {
                price: fill_price,
                qty: fill_qty,
                maker: maker.owner,
                taker,
            });
        }

        // 剩余挂入己方簿
        let resting = if order.qty > 0 {
            let seq = self.next_seq;
            self.next_seq += 1;
            order.seq = seq;
            self.insert_resting(order.clone());
            Some(order)
        } else {
            None
        };

        Ok(MatchResult { trades, resting })
```
> 说明：`crossed` 对卖单用 `order.price >= bid_p`（卖价 ≤ 买价 ⇔ 买价 ≥ 卖价，等价判断统一为「新单价 ≥ 对手最优价」）。`first_entry`/`pop_first` 在 stable Rust 1.96 可用（BTreeMap 已稳定）。

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（5 个撮合测试全过：一对一/部分成交/价格优先/时间优先/maker价）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/orderbook.rs packages/engine/tests/orderbook.rs
git commit -m "feat(engine): orderbook 撮合核心(price-time优先/maker价/部分成交)"
```

---

## Task 6: `cancel` 撤单

**Files:**
- Modify: `packages/engine/src/orderbook.rs`
- Modify: `packages/engine/tests/orderbook.rs`

**Interfaces:**
- Produces: `OrderBook::cancel(&mut self, id: OrderId) -> Result<Order, OrderError>`。遍历 bids/asks 找匹配 id，移除并返回；找不到 → `OrderNotFound`。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/orderbook.rs`：
```rust
#[test]
fn cancel_removes_and_returns_order() {
    let mut book = mk_book();
    book.place(buy(1, 1000, 100, 10)).unwrap();
    assert_eq!(book.best_bid(), Some(Money::from_cents(1000)));

    let removed = book.cancel(OrderId(1)).unwrap();
    assert_eq!(removed.id, OrderId(1));
    assert_eq!(removed.qty, 100);
    assert!(book.best_bid().is_none()); // 撤后空簿

    // 再次撤同 id → NotFound
    let err = book.cancel(OrderId(1)).unwrap_err();
    assert!(matches!(err, OrderError::OrderNotFound { .. }));
}

#[test]
fn cancel_unknown_id_errors() {
    let mut book = mk_book();
    assert!(matches!(
        book.cancel(OrderId(999)).unwrap_err(),
        OrderError::OrderNotFound { .. }
    ));
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `no method cancel`（RED）。

- [ ] **Step 3: 写实现**

在 `impl OrderBook` 内追加：
```rust
    /// 按 id 撤单，返回被撤订单（供上层回写）。不存在 → Err。
    pub fn cancel(&mut self, id: OrderId) -> Result<Order, OrderError> {
        // 买盘
        if let Some(key) = self.bids.iter().find(|(_, o)| o.id == id).map(|(k, _)| *k) {
            return Ok(self.bids.remove(&key).expect("key just found"));
        }
        // 卖盘
        if let Some(key) = self.asks.iter().find(|(_, o)| o.id == id).map(|(k, _)| *k) {
            return Ok(self.asks.remove(&key).expect("key just found"));
        }
        Err(OrderError::OrderNotFound(id))
    }
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/orderbook.rs packages/engine/tests/orderbook.rs
git commit -m "feat(engine): OrderBook::cancel 撤单 + NotFound"
```

---

## Task 7: 盘口深度 `bid_depth`/`ask_depth` + 收尾导出 + clippy 清零

**Files:**
- Modify: `packages/engine/src/orderbook.rs`
- Modify: `packages/engine/src/lib.rs`（re-export）
- Modify: `packages/engine/tests/orderbook.rs`

**Interfaces:**
- Produces: `OrderBook::bid_depth(&self) -> Vec<(Money, u32)>`（按价优→劣，每价聚合总量）；`ask_depth(&self) -> Vec<(Money, u32)>`。lib.rs re-export `{OrderBook, Order, Trade, Side, OrderId, OrderError, MatchResult}`。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/orderbook.rs`：
```rust
#[test]
fn depth_aggregates_by_price() {
    let mut book = mk_book();
    // 卖盘：10.00×100, 10.00×50(同价应聚合=150), 10.01×80
    book.place(sell(1, 1000, 100, 10)).unwrap();
    book.place(sell(2, 1000, 50, 11)).unwrap();
    book.place(sell(3, 1001, 80, 12)).unwrap();
    let ask_d = book.ask_depth();
    assert_eq!(ask_d[0], (Money::from_cents(1000), 150)); // 10.00 聚合 150
    assert_eq!(ask_d[1], (Money::from_cents(1001), 80));  // 10.01
    assert_eq!(book.bid_depth().len(), 0); // 无买单
}

#[test]
fn reexport_from_crate_root() {
    use engine::{OrderBook, Order, Trade, Side, OrderId};
    let _ = OrderBook::new(Money::from_cents(1)).unwrap();
    let _: Side = Side::Buy;
    let _: OrderId = OrderId(1);
    let _ = Trade { price: Money::ZERO, qty: 1, maker: engine::orderbook::AccountId(0), taker: engine::orderbook::AccountId(0) };
    let _: Option<Order> = None;
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `no method ask_depth` + re-export 缺失（RED）。

- [ ] **Step 3: 写实现**

在 `impl OrderBook` 内追加：
```rust
    /// 买盘深度（按价高→低，每价聚合总量）。
    pub fn bid_depth(&self) -> Vec<(Money, u32)> {
        self.aggregate(&self.bids, true)
    }

    /// 卖盘深度（按价低→高，每价聚合总量）。
    pub fn ask_depth(&self) -> Vec<(Money, u32)> {
        self.aggregate(&self.asks, false)
    }

    fn aggregate<T: Ord>(
        &self,
        side: &BTreeMap<(T, u64), Order>,
        _is_bid: bool,
    ) -> Vec<(Money, u32)> {
        let mut out: Vec<(Money, u32)> = Vec::new();
        for o in side.values() {
            if let Some(last) = out.last_mut() {
                if last.0 == o.price {
                    last.1 += o.qty;
                    continue;
                }
            }
            out.push((o.price, o.qty));
        }
        out
    }
```

`packages/engine/src/lib.rs` 追加 re-export（替换 Task 1 的 `pub mod orderbook;` 为）：
```rust
pub mod orderbook;
pub use orderbook::{OrderBook, Order, Trade, Side, OrderId, OrderError, MatchResult};
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS。

- [ ] **Step 5: clippy 清零**

Run: `cargo clippy -p engine --all-targets -- -D warnings`
Expected: 零告警。若有（如 `needless_range_loop`、`clippy::manual_map` 等）按 clippy 建议修，重跑清零。
> 关注：`match side { Buy => ..., Sell => ... }` 两分支若重复结构可能触发 clippy，可重构提取公共逻辑。`aggregate` 的 `_is_bid` 若未用会被 clippy 警告 → 删除该参数（两侧 BTreeMap 的遍历顺序已天然给出价优→劣，因为买盘 key 是 Reverse(price) 故 first 是最高价，卖盘 key 是 price 故 first 是最低价）。

**修正实现**（去掉无用 `_is_bid`）：
```rust
    pub fn bid_depth(&self) -> Vec<(Money, u32)> {
        self.aggregate(&self.bids)
    }
    pub fn ask_depth(&self) -> Vec<(Money, u32)> {
        self.aggregate(&self.asks)
    }
    fn aggregate<T: Ord>(&self, side: &BTreeMap<(T, u64), Order>) -> Vec<(Money, u32)> {
        let mut out: Vec<(Money, u32)> = Vec::new();
        for o in side.values() {
            if let Some(last) = out.last_mut() {
                if last.0 == o.price {
                    last.1 += o.qty;
                    continue;
                }
            }
            out.push((o.price, o.qty));
        }
        out
    }
```

- [ ] **Step 6: 全量回归**

Run: `cargo test -p engine`
Expected: 全绿（money 20 + config 28 + orderbook 新增，应 ≥ 56）。

Run: `cargo build -p engine`
Expected: 无 warning。

- [ ] **Step 7: 提交**

```bash
git add packages/engine/src/orderbook.rs packages/engine/src/lib.rs packages/engine/tests/orderbook.rs
git commit -m "feat(engine): orderbook 盘口深度 + 导出 + clippy 清零 + 全量回归绿"
```

---

## Self-Review（plan 作者自检）

**1. Spec 覆盖：**
- §3 类型（Side/Order/Trade/MatchResult/OrderBook）→ Task 1/2/3 ✓
- §3 方法（new/place/cancel/best_bid/best_ask/depth）→ Task 3/4/5/6/7 ✓
- §4 撮合算法（交叉/部分成交/price-time/maker价）→ Task 5 ✓
- §5 错误处理（InvalidPrice/InvalidQty/OrderNotFound/InvalidTick）→ Task 1/3/4/6 ✓
- §7 测试矩阵 11 项 → 散布 Task 1-7 ✓（构造+tick/无对手盘/一对一/部分/价格优先/时间优先/maker价/撤单/tick校验/盘口/serde）

**2. 占位扫描：** 无 TBD/TODO；每步含完整代码或命令 ✓。

**3. 类型一致性：**
- `Order { id, side, price, qty, owner, seq }` 字段名跨 Task 2/4/5/6 一致 ✓。
- `Trade { price, qty, maker, taker }`、`MatchResult { trades, resting }` 一致 ✓。
- `OrderBook` 内部 `bids: BTreeMap<(Reverse<Money>,u64),Order>` / `asks: BTreeMap<(Money,u64),Order>` 跨 Task 3-7 一致 ✓。
- `OrderError` 五变体签名（InvalidPrice{price,tick,reason}/InvalidQty(u32)/DuplicateOrderId(OrderId)/OrderNotFound(OrderId)/InvalidTick{tick}）跨任务一致 ✓。

**已知风险（实现期注意，非 plan 缺陷）：**
- `BTreeMap::pop_first` / `first_entry` 在 Rust 1.96 已稳定（本机 cargo 1.96）✓。
- 卖单 crossed 判断用 `order.price >= bid_p`（与买单对称），实现时按 Task 5 Step 3 注释。
