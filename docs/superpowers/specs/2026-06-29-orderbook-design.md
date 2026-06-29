# 设计：engine `orderbook` 模块 —— 价格-时间优先的限价撮合

- **日期 (Date):** 2026-06-29
- **状态 (Status):** 已批准（设计稿，待 TDD 实现）
- **决策者 (Deciders):** msuad + Claude
- **关联 (Related):** [ADR-0005](../../decisions/0005-unified-engine-three-deployments.md) §3（撮合驱动价格）、[ADR-0006](../../decisions/0006-npc-strategy-module.md)（策略产 Intent）、[`money` 设计](2026-06-29-money-fixed-point-design.md)。

---

## 1. 背景与动机

ADR-0005 §3 定调：**价格由撮合内生**——所有账户（含 NPC 与玩家）的挂单进共享 orderbook，买卖价交叉处成交。这取代了 ref 的「价格随机游走 + 即时按现价成交」。orderbook 是市场的心脏、撮合驱动的执行核心，且**只依赖 `Money`**（逻辑最纯粹、TDD 最顺），是市场/账户模块的地基。

本模块只做「单只股票的撮合引擎」：接收挂单 → 与对手方挂单撮合 → 产出成交记录。**不**关心：账户资金扣减（account 层）、行情驱动/价格序列（market 层）、NPC 决策（strategy 层）。解耦后可独立单测。

## 2. 核心决策

1. **价格-时间优先（FIFO 同价位）**：撮合时，买方取「出价最高、且出价最早」；卖方取「要价最低、且最早」。同价位先挂先成交（真实交易所规则）。
2. **限价单（limit order）**：本模块**只**支持限价单。买单 `price` 是「最多愿付」；卖单 `price` 是「最少愿收」。市价单等延后（YAGNI）。
3. **交叉即成交**：新单进入时，若能与对手盘最优挂单成交（买价 ≥ 卖价），立即撮合；可**部分成交**，剩余量挂入簿中等待。
4. **价格 = `Money`（分）**；**tick size 可配**（默认 0.01 元 = 1 分）——价格必须为 tick 整数倍，否则拒绝（防御式）。
5. **订单 ID 单调递增**：用于撤单（Cancel）定位 + 成交事件引用。本模块内自增分配。
6. **纯逻辑、无 I/O、无定时器**：`match_order(book, order) -> MatchResult` 是纯函数式推进；调用方（market/session）决定何时喂单。

## 3. 类型与 API

```rust
/// 买卖方向。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Side { Buy, Sell }

/// 单笔挂单（限价）。immutable 快照；内部用 OrderId 引用。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Order {
    pub id: OrderId,
    pub side: Side,
    pub price: Money,     // 限价（分）
    pub qty: u32,          // 剩余量（股；>0）
    pub owner: AccountId,  // 挂单方（用于成交后回写账户）
    pub seq: u64,          // 时间序：同价位排序键（先挂先成交）
}

/// 一笔成交（fill）。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Trade {
    pub price: Money,       // 成交价（取「被动方/较早挂单」的价格，真实交易所规则）
    pub qty: u32,           // 成交量
    pub maker: AccountId,   // 被动方（较早挂单、提供流动性者）
    pub taker: AccountId,   // 主动方（新进入、吃流动性者）
}

/// 撮合结果。
pub struct MatchResult {
    pub trades: Vec<Trade>,      // 本次撮合产生的成交（按发生顺序）
    pub resting: Option<Order>,  // 新单若有剩余量，挂入簿的残留订单（None=全成交或撤销）
}
```

**`OrderBook` 结构（每只股票一个）：**
```rust
/// 单只股票的订单簿。买盘按「价高优先、同价先挂优先」；卖盘按「价低优先、同价先挂优先」。
pub struct OrderBook {
    bids: BTreeMap<(Reverse<Money>, u64), Order>,  // 买：价格降序 + seq 升序
    asks: BTreeMap<(Money, u64), Order>,           // 卖：价格升序 + seq 升序
    next_seq: u64,                                  // 下一个挂单的 seq
    next_id: OrderId,                               // 下一个订单 id
    tick: Money,                                    // 价格最小变动
}
```

**方法：**
- `OrderBook::new(tick: Money) -> Self` —— tick 必须为正（否则 Err）。
- `book.place(order: Order) -> Result<MatchResult, OrderError>` —— 撮合新单。校验 price/qty 合法、price 为 tick 整数倍；与对手盘最优逐档撮合，产出 trades；剩余挂入己方簿。
- `book.cancel(id: OrderId) -> Result<Order, OrderError>` —— 撤单，返回被撤订单（用于上层回写）。不存在 → Err。
- `book.best_bid() -> Option<Money>` / `book.best_ask() -> Option<Money>` —— 盘口最优价（供 market 行情、NPC 的 MarketView）。
- `book.bid_depth()` / `book.ask_depth()` —— 各档挂单（深度，供 MarketView）。

## 4. 撮合算法（price-time）

`place(new)` 流程：
1. 校验 `new.price`（≥0、为 tick 整数倍）、`new.qty > 0`。非法 → `OrderError`。
2. 按 `new.side` 选对手盘（买单对 asks，卖单对 bids）。
3. 循环取对手盘**最优档**（ask 最低 / bid 最高）：
   - 若**无交叉**（买价 < 对手最优卖价 / 卖价 > 对手最优买价）→ 停止。
   - 否则成交：`fill_qty = min(new.qty, maker.qty)`；`fill_price = maker.price`（被动方价）；`new.qty -= fill_qty`；`maker.qty -= fill_qty`；记一笔 `Trade`。maker 清零则移出簿。
4. 若 `new.qty > 0` → 分配 seq，挂入己方簿，`resting = Some(new)`；否则 `resting = None`。

> 成交价取**被动方（较早挂单 maker）的价格**——真实交易所「price priority of the resting order」规则，也是买卖价差归属流动性的体现。

## 5. 错误处理（铁律二）

`OrderError`（thiserror）：
- `InvalidPrice { price, tick, reason }` —— 价格为负 / 非 tick 整数倍 / 超出 i64 分合理范围。
- `InvalidQty { qty }` —— qty == 0。
- `DuplicateOrderId { id }` —— 重复 id（防御式，理论上自增不会触发）。
- `OrderNotFound { id }` —— 撤单时 id 不存在。
- `InvalidTick { tick }` —— 构造时 tick ≤ 0。
**绝不**静默截断价格/数量、绝不产生负剩余量。

## 6. 不在本模块内（明确边界）

- 账户资金/持仓扣减与校验（资金不足、持仓不足）→ `account` 层。orderbook 只管撮合，不知道「账户有没有钱」。
- 涨跌停 clamp → `market` 层（orderbook 接收已 clamp 后的合法价格，或 market 在 place 前校验）。
- 行情驱动、tick 步进、隐藏公允价 V → `market` 层。
- 市价单 / IOC / FOK 等高级订单类型 → 延后（YAGNI）。

## 7. 测试矩阵（TDD：先红后绿）

`packages/engine/tests/orderbook.rs`：
1. **构造 + tick 校验**：`new(positive tick)` Ok；`new(Money::ZERO)` → `Err(InvalidTick)`。
2. **单边挂单无成交**：挂一买单（无对手盘）→ `trades` 空、`resting = Some`、`best_bid` = 该价。
3. **交叉即成交（一对一）**：先挂卖 10.00×100，再挂买 10.00×100 → 1 笔 trade，价 10.00、量 100、maker=卖方、taker=买方；两单均不在簿（resting=None）。
4. **部分成交**：买 10.00×100 vs 卖 10.00×250 → 1 笔 trade 量 100；卖单剩余 150 留在簿；买单 resting=None。
5. **价格优先（吃更优的对手单）**：卖盘有 ask 10.01×100、10.00×100；新买单 10.01×150。买 10.01 可成交的对手是 ask ≤ 10.01，**最优 ask=10.00（卖方更低=对买方更优）先成交**，再吃 10.01。→ 2 笔 trade，成交价分别为 10.00 / 10.01，量各 100 / 50（买单总量 150 用尽）。
6. **时间优先（同价位先挂先成交）**：两个卖单同价 10.00，先挂 A 后挂 B；买单吃该价 → 先成交 A（seq 小）。
7. **成交价 = 被动方价**：买 10.05 吃卖 10.00 → 成交价 10.00（被动方/较早挂单的价），非 10.05。
8. **撤单**：挂单后 cancel(id) 返回该单、best 价随之更新；再次 cancel 同 id → `Err(OrderNotFound)`。
9. **tick 校验（非法价格）**：tick=0.01，place 价格 10.005（非整数倍）→ `Err(InvalidPrice)`；qty=0 → `Err(InvalidQty)`。
10. **盘口只读访问**：best_bid/best_ask/depth 反映簿状态（空簿 → None）。
11. **serde 往返**：Order/Trade 可序列化（前端/存档契约）。

## 8. 文件布局

```
packages/engine/src/
├── lib.rs            # 追加 `pub mod orderbook;` + re-export {OrderBook, Order, Trade, Side, OrderError}
└── orderbook.rs      # 类型 + 撮合算法（BTreeMap 实现 price-time）
packages/engine/tests/
└── orderbook.rs      # 集成测试
```

> `OrderId` / `AccountId` 类型：本模块先用简单 newtype（`pub struct OrderId(pub u64)` / `AccountId` 暂用 `pub struct AccountId(pub u64)` 或字符串）——待 `account` 模块落地后统一。本模块不引入对 account 的依赖（保持解耦），用轻量 newtype 占位。

## 9. 验收标准（Definition of Done）

- `cargo test -p engine` 全绿（money 20 + config 28 + orderbook 新增）。
- `cargo clippy -p engine --all-targets -- -D warnings` 零告警。
- `cargo build -p engine` 无 warning。
- 源码审计：撮合无非负剩余量、无静默吞错、价格全程 `Money`（无 f64 存储价）、price-time 顺序正确。
- `lib.rs` 导出 orderbook 公共类型。
- Conventional Commits，scope `engine`。
