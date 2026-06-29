# 设计：engine `market` 模块 —— 单只股票的市场状态层

- **日期 (Date):** 2026-06-29
- **状态 (Status):** 已批准（设计稿，待 TDD 实现）
- **决策者 (Deciders):** msuad + Claude
- **关联 (Related):** [ADR-0005](../../decisions/0005-unified-engine-three-deployments.md) §3（撮合驱动价格 + 隐藏公允价 V）、[ADR-0006](../../decisions/0006-npc-strategy-module.md)（机构 ValueStrategy 读 V）、[`orderbook` 设计](2026-06-29-orderbook-design.md)、[`account` 设计](2026-06-29-account-design.md)、[`money` 设计](2026-06-29-money-fixed-point-design.md)。

---

## 1. 背景与动机

ADR-0005 §3 定调「撮合驱动价格 + 隐藏公允价 V」。`orderbook`（撮合）+ `account`（账务）已就位，但缺一个把**单只股票的全部市场状态**收口的容器：最近成交价、昨收(涨跌停基准)、涨跌幅、隐藏的「真实价值」V。`market` 就是这个容器——每只股票一个，包装一个 `OrderBook`，叠加涨跌停校验、价格记录、V 演化、日终重置。

**范围（ADR-0005 已明确）：** `market` 只做**单只股票的市场状态**，**不含**全 tick 编排（NPC 调用 → 下单 → 撮合 → account 结算）。编排属 `session`/`simulator` 层（下一批）。本模块解耦、可独立单测，与既有风格一致（orderbook 不知 account、account 不知 orderbook 内部；market 同样不碰 account，`place` 返回 `MatchResult` 由编排层结算）。

隐藏 V 是机构 `ValueStrategy` 的依据（ADR-0006）：玩家不可见、机构据此低买高卖。V 的演化纳入 Session 的种子化 RNG（可重放、可单测）。

## 2. 核心决策（含 msuad 2026-06-29 拍板）

1. **涨跌停 = 拒单**（msuad 定，贴 A 股）：`place` 时若 `order.price` 超出 `[down_stop, up_stop]` → `Err(MarketError::LimitExceeded)`，**不挂单、不撮合**。只有涨停价买单、跌停价卖单（边界价）接受。
2. **V = 几何均值回复随机游走**（msuad 定）：`V_new = V × (1 + drift)`，`drift = α·(长期均值 − V)/V + σ·z`（z 为种子化随机）；向长期均值回复防发散，**V 始终为正值 Money**。
3. **价格全程 `Money`**：last_price / last_close / up_stop / down_stop / V 均为定点 `Money`（i64 分）。V 演化的 f64 仅在「乘 drift → 银行家舍入到分」这一边界出现（复用 money 模块约定，绝不存 f64 权威状态）。
4. **last_price 由成交驱动**：每笔成交（fill）的 price 成为新的 `last_price`；无成交则不变。
5. **place 委托 book**：涨跌停校验通过后，调用 `book.place(order)`，返回其 `MatchResult`；用末笔 trade 更新 last_price。
6. **纯逻辑、无 I/O、无定时器**：`evolve_v(&mut dyn Rng)` / `end_of_day()` 是状态推进；何时调用由编排层决定。

## 3. 类型与 API

```rust
/// 单只股票的市场状态（每只股票一个实例）。
pub struct Market {
    code: StockCode,
    book: OrderBook,            // 复用撮合引擎
    last_price: Money,          // 最近成交价
    last_close: Money,          // 昨收（涨跌停基准）
    fundamental_value: Money,   // 隐藏公允价 V（机构依据，玩家不可见）
    limit_pct: f64,             // 涨跌幅（0.10/0.05，来自 GameConfig）
}

/// market 操作失败。绝不静默吞掉（铁律二）。
#[derive(Debug, thiserror::Error)]
pub enum MarketError {
    /// 下单价超出涨跌停范围。
    #[error("limit exceeded for {code:?}: price {price:?} not in [{down:?}, {up:?}]")]
    LimitExceeded { code: StockCode, price: Money, down: Money, up: Money },
    /// 透传 orderbook 错误（非法价格/数量/tick）。
    #[error(transparent)]
    OrderBook(#[from] OrderError),
    /// 透传 money 错误（涨跌停价/apply_rate 溢出）。
    #[error(transparent)]
    Money(#[from] MoneyError),
    /// V 演化参数非法（volatility/mean_reversion 负或非有限）。
    #[error("invalid v params: {reason}")]
    InvalidVParams { reason: String },
}
```

**方法：**
- `Market::new(code, initial_price, limit_pct, v_initial, tick) -> Result<Market, MarketError>` —— 构造；校验 `limit_pct ∈ (0,1)`、`initial_price/v_initial > 0`、tick 合法（交 book 校验）。`last_close = last_price = initial_price`（首日无昨收，以开盘价为准）。
- `up_stop() / down_stop() -> Money` —— `last_close.apply_rate(1.0 + limit_pct)` / `last_close.apply_rate(1.0 - limit_pct)`（银行家舍入到分）。
- `place(&mut self, order: Order) -> Result<MatchResult, MarketError>` —— 涨跌停校验（price ∈ [down_stop, up_stop]，否则 `LimitExceeded`）→ `book.place(order)` → 末笔 trade 更新 `last_price` → 返回 MatchResult。
- `evolve_v(&mut self, params: &VParams, rng: &mut dyn Rng) -> Result<(), MarketError>` —— V 几何均值回复一步：校验 params；`drift = α·(mean − V)/V + σ·z`；`V_new = round_half_to_even(V_cents as f64 × (1+drift))`；防 V≤0（clamp 到 1 分并报错？**约定：若结果 ≤0 → Err(InvalidVParams{reason:"v went non-positive"})**，绝不静默存非正值）。
- `end_of_day(&mut self)` —— `last_close = last_price`；为次日重置涨跌停基准。（V、book 跨日保留；可选日终也 evolve V。）
- `code()/last_price()/last_close()/fundamental_value()/best_bid()/best_ask()/bid_depth()/ask_depth()` —— 只读访问。
- `book()` / `book_mut()` —— 受控暴露内部 orderbook（供编排层 cancel 等）。

**`VParams`（V 演化参数，可配，来自 GameConfig 或自带）：**
```rust
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct VParams {
    pub long_run_mean: Money,  // 长期均值（V 向其回复）
    pub mean_reversion: f64,   // α：回复速度（≥0）
    pub volatility: f64,       // σ：随机扰动幅度（≥0）
}
```

## 4. 涨跌停与 V 演化逻辑

**涨跌停价**（每 tick 可重算，因 last_close 日内不变）：
- `up_stop = last_close × (1 + limit_pct)`
- `down_stop = last_close × (1 − limit_pct)`
- 校验：`down_stop ≤ price ≤ up_stop`（闭区间，边界价合法）。

**V 几何均值回复**（一步）：
1. 校验 `mean_reversion ≥ 0`、`volatility ≥ 0`、均有限；`long_run_mean > 0`、`V > 0`。
2. `z = rng.next_f64()*2 − 1`（[−1,1]，或更标准的正态——简化为均匀乘系数）。
3. `gap = (long_run_mean.cents() as f64 − V_cents) / V_cents`（偏离比）。
4. `drift = mean_reversion × gap + volatility × z`。
5. `multiplier = 1.0 + drift`；若 `multiplier ≤ 0` → Err（防 V 跨零）。
6. `V_new_cents = round_half_to_even(V_cents as f64 × multiplier)`；若 ≤0 → Err。
7. `self.fundamental_value = Money::from_cents(V_new_cents)`。

> 复用 money 模块的 `round_half_to_even`（私有 fn）——或 market 自带一份相同实现（避免跨模块私有依赖）。**约定：market 自带 `round_half_to_even_i64`**（与 account.rs 同实现，保持模块自洽）。

## 5. 错误处理（铁律二）

- 涨跌停超限 → `LimitExceeded{code, price, down, up}`（玩家可见「已涨停/跌停」）。
- orderbook/money 错误透传（`#[from]`）。
- V 参数非法 / V 跨零 → `InvalidVParams`。
- **绝不**静默 clamp 价格、绝不静默存非正 V、绝不吞 book/money 错误。

## 6. 不在本模块内（明确边界）

- 每 tick 编排（NPC 决策 → 下单 → 撮合 → account 结算）→ `session`/`simulator`（下一批）。
- account 资金/持仓结算 → `account`（market 只产 MatchResult）。
- NPC 策略算法 → `strategy`（market 只暴露 V/盘口供其 MarketView）。
- 多股票集合 / Session 抽象 → `session`（下一批）。
- 真实交易日的集合竞价/连续竞价阶段差异 → 延后（YAGNI，本模块只提供 `end_of_day` 钩子）。

## 7. 测试矩阵（TDD：先红后绿）

`packages/engine/tests/market.rs`：
1. **构造 + 校验**：`new` 合法 → last_price/last_close/V/limit 正确；`limit_pct ∉ (0,1)` → Err；`initial_price ≤ 0` → Err。
2. **涨跌停价**：last_close=1000 分、limit=0.10 → up_stop=1100、down_stop=900（apply_rate 银行家舍入）。
3. **place 涨跌停拒单**：up_stop=1100 时，买单 price=1101 → `Err(LimitExceeded{..})`，**book 不变**；price=1100（边界）→ 接受。
4. **place 成交更新 last_price**：无对手盘挂买单 1000 → last_price 不变（无成交）；两单交叉成交 → last_price = 末笔 trade 价。
5. **place 委托 book 撮合**：market.place 产出的 MatchResult 与直接 book.place 一致（涨跌停内的撮合逻辑复用）。
6. **evolve_v 均值回复**：固定种子 RNG，V=1200、long_run_mean=1000、α=0.5、σ=0 → V 向 1000 回复（drift 负、V 减小）；断言 V_new < 1200 且 >0。
7. **evolve_v 波动**：σ>0 + 固定种子 → V 改变方向可断言（确定性）。
8. **evolve_v 非法参数**：volatility<0 / mean_reversion<0 / long_run_mean≤0 → `Err(InvalidVParams)`。
9. **evolve_v 防跨零**：极端 drift 使 multiplier≤0 → Err，**V 不变**（不存非正值）。
10. **end_of_day**：last_price=1050 → end_of_day 后 last_close=1050、up_stop/down_stop 基准更新。
11. **只读访问**：code/last_price/last_close/fundamental_value/best_bid/best_ask 正确反映状态。
12. **V 对玩家隐藏的契约**：`fundamental_value()` 存在但调用方（玩家 UI）不应读——契约靠文档/类型（本模块提供方法，编排层决定谁看）。

## 8. 文件布局

```
packages/engine/src/
├── lib.rs          # 追加 `pub mod market;` + re-export
└── market.rs       # Market/MarketError/VParams + 涨跌停/V演化逻辑
packages/engine/tests/
└── market.rs       # 集成测试
```

> 复用：`engine::orderbook::{OrderBook, Order, Trade, Side, OrderId, OrderError, MatchResult, AccountId}`；`engine::money::{Money, MoneyError}`（`apply_rate`/`from_cents`/`cents`/`ZERO`）；`engine::strategy::Rng`（V 演化的随机源）；`StockCode` 来自 account 模块（或 market 自带——**约定复用 account::StockCode**，统一 newtype）。

## 9. 验收标准（Definition of Done）

- `cargo test -p engine` 全绿（money 20 + config 28 + orderbook 15 + account 18 + market 新增，应 ≥ 95）。
- `cargo clippy -p engine --all-targets -- -D warnings` 零告警。
- `cargo build -p engine` 无 warning。
- 源码审计：价格/V 全程 Money（i64 分）、f64 仅在 V 演化边界 + 银行家舍入、涨跌停超限显式拒单无静默 clamp、V 防跨零、book/money 错误透传。
- `lib.rs` 导出 market 公共类型。
- Conventional Commits，scope `engine`。

## 10. 实现期风险与边界

- **`round_half_to_even_i64`**：market 自带一份（与 account.rs 同实现，纯整数 div_euclidean/rem_euclid），不跨模块依赖私有 fn。
- **V 演化的确定性测试**：用固定种子的 mock `Rng`（测试内实现 `Rng` trait 返回固定序列），断言 V_new 精确值。
- **`StockCode` 复用**：`account::StockCode` 已是 `pub struct StockCode(pub String)`；market 直接 `use crate::account::StockCode`，避免重复定义。
- **涨跌停边界**：闭区间 `[down_stop, up_stop]`，边界价（涨停价买/跌停价卖）合法。
