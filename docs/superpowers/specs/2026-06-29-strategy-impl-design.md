# 设计：engine `strategy` 三策略实现 + 工厂（多股/NPC）

- **日期 (Date):** 2026-06-29
- **状态 (Status):** 已批准（设计稿，待 TDD 实现）
- **决策者 (Deciders):** msuad + Claude
- **关联 (Related):** [ADR-0006](../../decisions/0006-npc-strategy-module.md)、[ADR-0005](../../decisions/0005-unified-engine-three-deployments.md)（统一账户、隐藏 V）、[`strategy` trait 骨架](2026-06-29-account-design.md) §strategy。

---

## 1. 背景与动机

ADR-0006 定调策略为「独立 trait 模块 + 每实例独立参数 + 可插拔」。`strategy` trait 骨架（`Strategy::decide`/`Intent`/`MarketView`/`Rng`）已在 account 批次落地。本批次实现**首批三策略**（散户 ZiNoise / 机构 Value / 游资 Momentum）+ 工厂，使 NPC 能真正产生下单意图。

msuad 2026-06-29 拍板 **「多股/NPC」**：每个 NPC 可交易多只股票。这要求演化骨架（非推翻 ADR，是补全缺口）：
1. `Intent` 加 `code: StockCode`（指定哪只股票）。
2. `MarketView`/`SelfView` 变多股（map）。
3. `decide` 返回 `Vec<Intent>`（每 tick 可对多股产生 0..N 个动作）。
4. 需要价格历史（游资趋势检测）→ `StockView` 带 `recent_prices`。

## 2. 核心决策

1. **多股视图**：`MarketView.stocks: BTreeMap<StockCode, StockView>`；`SelfView` = `cash` + `positions: BTreeMap<StockCode, PositionView>`。
2. **V 可见性**：`StockView.fundamental_value: Option<Money>`——`Some` 仅对该策略可见（编排层决定：机构 Some、散户/游资/玩家 None）。玩家永不读 V。
3. **Intent 带 code**：`PlaceLimit{code, side, price, qty}` / `PlaceMarket{code, side, qty}` / `Cancel{code, id}`。**移除 `Pass` 变体**——空 `Vec<Intent>` 即 pass。
4. **decide 返回 Vec<Intent>**：每 tick 0..N 个动作（多股）。
5. **价格历史在视图**：`StockView.recent_prices: Vec<Money>`（最近 N 个 last_price，由编排层维护滚动窗口）——游资趋势检测用，避免策略自带历史状态。
6. **每实例独立参数**：策略 struct 自带参数；工厂从分布采样。
7. **纯逻辑、种子化**：所有随机经注入的 `&mut dyn Rng`，可重放、可单测。

## 3. 类型与 API（演化骨架）

```rust
/// 单只股票的市场视图（多股 MarketView 的元素）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StockView {
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
    pub last_price: Money,
    pub fundamental_value: Option<Money>, // V；Some 仅对该策略可见
    pub recent_prices: Vec<Money>,        // 最近 N 个 last_price（滚动窗口）
}

/// 整个市场快照（多股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MarketView {
    pub stocks: BTreeMap<StockCode, StockView>,
}

/// 策略所属账户的自身快照（跨股）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SelfView {
    pub cash: Money,
    pub positions: BTreeMap<StockCode, PositionView>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PositionView {
    pub qty: u32,            // 总持仓
    pub sellable_qty: u32,   // 可卖（qty − T+1锁定）
    pub cost_price: Option<Money>,
}

/// 策略决策产物。code 指定股票；空 Vec = pass。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Intent {
    PlaceLimit { code: StockCode, side: Side, price: Money, qty: u32 },
    PlaceMarket { code: StockCode, side: Side, qty: u32 },
    Cancel { code: StockCode, id: OrderId },
}

/// NPC 策略统一抽象（演化：多股视图 + Vec 返回）。
pub trait Strategy {
    fn decide(&mut self, market: &MarketView, own: &SelfView, rng: &mut dyn Rng) -> Vec<Intent>;
}
```

> 移除旧骨架的 `Intent::Pass`（用空 Vec）。`MarketView` 由「单股 3 字段」改为「多股 map」。account 测试中的旧 MarketView/Intent 构造需同步更新（Task 1）。

## 4. 三策略逻辑

### ZiNoiseStrategy（散户 retail）
- 每 tick：以概率 `arrival_rate` 决定是否动作。
- 若动作：从 `market.stocks` 随机选一只；side 50/50；qty ≈ `order_size_mean`（小额）；价格 = best_bid+tick（买）或 best_ask−tick（卖），无对手盘则 last_price 附近。
- 以概率 `chase_prob` 改为追势：recent_prices 末段升 → 买、降 → 卖。
- **参数**：`arrival_rate: f64`（0..1）、`order_size_mean: u32`、`chase_prob: f64`、`tick_cents: i64`（报价偏移单位）。

### ValueStrategy（机构 inst）
- 扫描所有 `fundamental_value` 为 `Some(V)` 的股票（机构可见 V）。
- 目标价 `target` 由 `target_policy` 定（**每实例一种**，机构看法各异）：
  - `Fixed(Money)`：固定目标价。
  - `TrackV { bias: f64 }`：`target = V × (1 + bias)`（跟随 V，带看法偏差）。
  - `DriftUp { rate: f64, base: Money }`：`target = base × (1 + rate × ticks_elapsed)`（认为大致上涨；ticks 由 decide 调用计数维护在 self）。
- 若 `last_price < target × (1 − margin)` 且现金足够 → 买（qty = `order_size`，大单可拆）。
- 若 `last_price > target × (1 + margin)` 且 `sellable_qty > 0` → 卖。
- 一次可对多只错定价股票产生多个 Intent。
- **参数**：`target_policy`、`margin: f64`、`order_size: u32`。

### MomentumStrategy（游资 hot）
- 扫描所有股票的 `recent_prices`（窗口 `lookback`）。
- 计算短期趋势：末价 vs 窗口首价，变化幅度 > `trend_threshold` → 趋势成立。
- 上涨趋势 + 可买（现金）→ 买；下跌趋势 + `sellable_qty > 0` → 卖。
- 快进快出（`holding_horizon` 短；本批次简化：不主动基于持仓时长平仓，靠趋势反转卖出）。
- **参数**：`lookback: usize`、`trend_threshold: f64`、`order_size: u32`。

## 5. 工厂 + 参数分布

```rust
/// 每类 NPC 的策略参数分布（从中为每个实例采样）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StrategyParams {
    pub retail: RetailParams,
    pub inst: InstParams,
    pub hot: HotParams,
}
// 各含分布参数（均值/范围）；省略具体字段，实现期定。

pub struct StrategyFactory;
impl StrategyFactory {
    /// 按种类从分布采样、构造一个策略实例。
    pub fn build(kind: AccountKind, params: &StrategyParams, rng: &mut dyn Rng) -> Option<Box<dyn Strategy>>;
    // Player → None（玩家无策略）
}
```

> 工厂采样用注入 RNG → 可重放、可单测（固定种子产出确定策略参数）。

## 6. 错误处理（铁律二）

- 策略本身**不返回 Result**——它只产 Intent；意图的**可行性校验**（资金/持仓/涨跌停）由编排层 + account + market 负责（已建）。策略产出的不可行 Intent 被编排层静默丢弃并**记录**（不静默吞——编排层须在事件流/日志体现「意图被拒」，避免无声丢失）。
- 参数非法（如 margin<0、arrival_rate∉[0,1]）→ 工厂构造时 `Err` 或返回 None + 上报；**绝不**静默用默认值。
- 策略内部随机计算不得 panic；NaN/Inf 由 RNG 实现保证不产出。

## 7. 不在本模块内（明确边界）

- 编排（每 tick 调 decide → 路由 Intent → market.place → account 结算）→ `session`/`simulator`（下一批）。
- 自身账户状态变更 → account（策略只读 SelfView）。
- 涨跌停/撮合 → market/orderbook（策略产 Intent，由 market 校验）。
- 玩家 Intent → UI 直接产（不经策略）。

## 8. 测试矩阵（TDD：先红后绿）

`packages/engine/tests/strategy.rs`：
1. **视图/Intent 构造 + serde**：MarketView/SelfView/StockView/PositionView/Intent(code) 构造与序列化往返。
2. **ZiNoise 决策**：固定种子 RNG + arrival_rate=1.0 → 必产生 ≥1 个 Intent；arrival_rate=0 → 空 Vec；选中的股票 code 在 market.stocks 内。
3. **ZiNoise 追势**：recent_prices 上升 + chase_prob=1 → 产生 Buy intent。
4. **Value 买入**：V=1000、target=TrackV{bias:0}、margin=0.05、last_price=900（<950）→ 产生 Buy；last_price=1000（在带内）→ 无动作。
5. **Value 卖出**：last_price=1100（>1050）+ SelfView 有 sellable → 产生 Sell；无持仓 → 无 Sell。
6. **Value 目标价策略**：Fixed/TrackV/DriftUp 三种 target_policy 算出不同 target（断言 target 值）。
7. **Momentum 趋势**：recent_prices 上升超 threshold → Buy；下降 → Sell（需持仓）；无趋势 → 无动作。
8. **V 不可见**：散户/游资的 StockView.fundamental_value=None 时，其逻辑不依赖 V（不 panic、行为合理）。
9. **工厂**：build(Retail) → Some(Box<ZiNoise>)；build(Player) → None；固定种子两次 build 产出相同参数的策略（可重放）。
10. **参数非法**：margin<0 / arrival_rate∉[0,1] → 工厂 Err 或 None。

## 9. 文件布局

```
packages/engine/src/
├── lib.rs          # 追加 `pub mod strategy_impl;`？——否：扩展现有 strategy.rs
├── strategy.rs     # 修改：演化骨架(MarketView多股/Intent带code/decide→Vec) + 三策略 + 工厂
└── (account.rs 不动，仅其测试的旧 MarketView 构造需更新)
packages/engine/tests/
├── strategy.rs     # 新建：三策略 + 工厂测试
└── account.rs      # 修改：更新旧 MarketView/Intent 构造（适配新签名）
```

> 决策：三策略放进**现有 `strategy.rs`**（同模块），不新建 crate/文件（YAGNI；文件尚不臃肿）。若 strategy.rs 过大再拆 `strategy/` 子模块。

## 10. 验收标准（Definition of Done）

- `cargo test -p engine` 全绿（money 20 + config 28 + orderbook 15 + account 18 + market 15 + strategy 新增，应 ≥ 110）。
- `cargo clippy -p engine --all-targets -- -D warnings` 零告警。
- `cargo build -p engine` 无 warning。
- 源码审计：策略纯逻辑无 f64 存储、随机全经注入 RNG（可重放）、参数非法显式拒绝、V 不可见时散户/游资不 panic。
- `lib.rs` 导出三策略 + 工厂 + 新视图类型。
- Conventional Commits，scope `engine`。

## 11. 实现期风险与边界

- **骨架演化破坏 account 测试**：account.rs 测试 `intent_and_marketview_construct` 用旧 3 字段 MarketView + Intent::Pass。Task 1 须同步更新该测试（适配多股 MarketView + 无 Pass）。这是 API 演化的预期成本，非弱化断言。
- **`Side`/`OrderId`/`StockCode`/`AccountKind` 复用**：来自 orderbook/account 模块。
- **固定种子 mock Rng**：测试内实现 `Rng` 返回确定序列，断言策略输出精确 Intent。
- **DriftUp 的 ticks 计数**：ValueStrategy 在 self 维护 `ticks: u64`，每次 decide 自增。
- **策略不持有对外部状态的引用**：纯参数 + decide 输入，无生命周期陷阱。
