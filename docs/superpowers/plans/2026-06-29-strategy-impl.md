# engine `strategy` 三策略 + 工厂 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: 用 ultracode workflow 串行 TDD（红→绿→提交），与既有模块同款，一路跑到全绿 + 独立验证门。Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 ADR-0006 首批三策略（散户 ZiNoise / 机构 Value / 游资 Momentum）+ 工厂，演化 strategy trait 骨架为「多股/NPC」（Intent 带 code、decide 返回 Vec<Intent>、MarketView/SelfView 多股）。

**Architecture:** Task 1 演化骨架类型（StockView/MarketView/SelfView/PositionView/Intent+code/decide→Vec）并修复 account 旧测试。Tasks 2-4 三策略（纯逻辑、注入 RNG、读视图）。Task 5 工厂（从分布采样每实例参数）。Task 6 clippy+导出+回归。策略只产 Intent，可行性校验由 session/account/market。

**Tech Stack:** Rust 2021，`std::collections::BTreeMap`，`serde`/`serde_json`/`thiserror`（workspace 已就位）。测试 `cargo test -p engine`。

## Global Constraints

- **铁律二（防御式）：** 参数非法（margin<0、arrival_rate∉[0,1]、lookback=0 等）→ 工厂/构造显式 `Err` 或 None + 上报，**禁止**静默用默认值；策略内部计算**禁止 panic**、禁止 NaN/Inf（RNG 实现保证）。
- **铁律一（TDD）：** 每任务严格 红→绿→提交；先写失败测试再写实现。
- 策略纯逻辑；**禁止 f64 存储权威状态**（f64 仅在计算 target/drift 边界，立即落回 Money；价格/qty 用 Money/u32）。
- 所有随机经注入的 `&mut dyn Rng`（`engine::strategy::Rng`），**可重放、可单测**（固定种子 mock）。
- 依赖只用 workspace 已声明的 `serde`/`serde_json`/`thiserror` + std；**不引入新依赖**。
- 复用：`engine::money::{Money}`、`engine::orderbook::{Side, OrderId}`、`engine::account::{StockCode, AccountKind}`、`engine::strategy::Rng`。
- 提交信息 Conventional Commits，scope `engine`，末尾 `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`。
- 命名/注释风格匹配 `packages/engine/src/{money,config,orderbook,account,market}.rs`（中文 doc 注释、serde derive）。

## File Structure

```
packages/engine/src/
├── lib.rs          # 修改：re-export 扩展（三策略 + 工厂 + 新视图类型）
└── strategy.rs     # 修改：演化骨架 + 三策略 + 工厂（同模块）
packages/engine/tests/
├── strategy.rs     # 新建：三策略 + 工厂测试（含固定种子 mock Rng）
└── account.rs      # 修改：更新旧 MarketView/Intent 构造（Task 1 适配新签名）
```

---

## Task 1: 演化骨架类型（多股视图 + Intent+code + decide→Vec）+ 修复 account 旧测试

**Files:**
- Modify: `packages/engine/src/strategy.rs`
- Modify: `packages/engine/tests/account.rs`
- Create: `packages/engine/tests/strategy.rs`

**Interfaces:**
- Produces（替换旧骨架）：`StockView{best_bid:Option<Money>,best_ask:Option<Money>,last_price:Money,fundamental_value:Option<Money>,recent_prices:Vec<Money>}`；`MarketView{stocks:BTreeMap<StockCode,StockView>}`；`SelfView{cash:Money,positions:BTreeMap<StockCode,PositionView>}`；`PositionView{qty:u32,sellable_qty:u32,cost_price:Option<Money>}`；`Intent{PlaceLimit{code,side,price,qty}/PlaceMarket{code,side,qty}/Cancel{code,id}}`（**移除 Pass**）；`Strategy::decide(&mut self,&MarketView,&SelfView,&mut dyn Rng)->Vec<Intent>`。保留 `Rng` trait 不变。
- Consumes: `Money`、`Side`、`OrderId`、`StockCode`。

- [ ] **Step 1: 先更新 account 旧测试（让它对新 API 编译），再写 strategy 新测试**

`packages/engine/tests/account.rs`：找到 `intent_and_marketview_construct` 测试，**整体替换**为适配新 API 的版本：
```rust
#[test]
fn intent_and_marketview_construct() {
    use engine::strategy::{Intent, MarketView, StockView, SelfView, PositionView};
    use engine::account::StockCode;
    use engine::orderbook::Side;
    use std::collections::BTreeMap;
    use engine::Money;

    let mut stocks = BTreeMap::new();
    stocks.insert(StockCode("600101".to_string()), StockView {
        best_bid: Some(Money::from_cents(999)),
        best_ask: Some(Money::from_cents(1001)),
        last_price: Money::from_cents(1000),
        fundamental_value: None,
        recent_prices: vec![Money::from_cents(1000)],
    });
    let mv = MarketView { stocks };
    assert_eq!(mv.stocks.len(), 1);

    let i = Intent::PlaceLimit { code: StockCode("600101".to_string()), side: Side::Buy, price: Money::from_cents(1000), qty: 100 };
    assert!(matches!(i, Intent::PlaceLimit { side: Side::Buy, qty: 100, .. }));

    let sv = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() };
    assert_eq!(sv.cash.cents(), 1_000_000);
    let _pv = PositionView { qty: 0, sellable_qty: 0, cost_price: None };
}
```
> 若 account.rs 顶部有 `use engine::strategy::{...}` 含旧类型名（如 `Strategy`），保留实际被 account 模块用到的（account.rs 持 `Box<dyn Strategy>`，需 `Strategy`）。检查并保留 `Strategy` 导入；移除已不存在的旧 `Intent::Pass` 引用。

`packages/engine/tests/strategy.rs`（新建）：
```rust
//! engine strategy 三策略 + 工厂集成测试（TDD 红绿循环）。
use engine::strategy::{Intent, MarketView, StockView, SelfView, PositionView, Rng};
use engine::account::StockCode;
use engine::money::Money;
use engine::orderbook::Side;
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
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误——strategy.rs 旧 `MarketView` 3 字段 / `Intent::Pass` 与新测试不符（RED）。

- [ ] **Step 3: 演化 strategy.rs 骨架**

`packages/engine/src/strategy.rs`：**替换** `Intent`、`MarketView`、`Strategy` 定义为：
```rust
use crate::account::StockCode;
use crate::money::Money;
use crate::orderbook::{OrderId, Side};
use std::collections::BTreeMap;

/// 单只股票的市场视图（多股 MarketView 的元素）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StockView {
    pub best_bid: Option<Money>,
    pub best_ask: Option<Money>,
    pub last_price: Money,
    /// 隐藏公允价 V；Some 仅对该策略可见（编排层决定：机构 Some、散户/游资/玩家 None）。
    pub fundamental_value: Option<Money>,
    /// 最近 N 个 last_price（滚动窗口，游资趋势检测用）。
    pub recent_prices: Vec<Money>,
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

/// 单只股票的持仓视图。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PositionView {
    pub qty: u32,
    pub sellable_qty: u32,
    pub cost_price: Option<Money>,
}

/// 策略决策产物。code 指定股票；空 Vec = pass。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Intent {
    PlaceLimit { code: StockCode, side: Side, price: Money, qty: u32 },
    PlaceMarket { code: StockCode, side: Side, qty: u32 },
    Cancel { code: StockCode, id: OrderId },
}

/// 随机源抽象（不变）。
pub trait Rng {
    fn next_f64(&mut self) -> f64;
    fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32;
}

/// NPC 策略统一抽象。看多股市场 + 自身快照 + RNG，返回 0..N 个 Intent。
pub trait Strategy {
    fn decide(&mut self, market: &MarketView, own: &SelfView, rng: &mut dyn Rng) -> Vec<Intent>;
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（account 旧测试已适配 + strategy 新 serde 测试通过；全 crate 此前 96 测试不回归）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/strategy.rs packages/engine/tests/account.rs packages/engine/tests/strategy.rs
git commit -m "refactor(engine): strategy 骨架演化为多股(Intent+code/decide→Vec/SelfView)"
```

---

## Task 2: `ZiNoiseStrategy`（散户）

**Files:**
- Modify: `packages/engine/src/strategy.rs`
- Modify: `packages/engine/tests/strategy.rs`

**Interfaces:**
- Produces: `pub struct ZiNoiseStrategy { arrival_rate:f64, order_size_mean:u32, chase_prob:f64, tick_cents:i64 }`；`impl Strategy for ZiNoiseStrategy`。`ZiNoiseStrategy::new(arrival_rate, order_size_mean, chase_prob, tick_cents) -> Result<Self, StrategyError>`（arrival_rate∈[0,1]、order_size_mean>0、chase_prob∈[0,1]、tick_cents>0）。
- 产生 `StrategyError`（本任务定义）。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/strategy.rs`：
```rust
use engine::strategy::{ZiNoiseStrategy, StrategyError, Strategy};

#[test]
fn zi_noise_arrival_rate_zero_produces_nothing() {
    let mut s = ZiNoiseStrategy::new(0.0, 100, 0.0, 1).unwrap();
    let mv = one_stock_view(1000, None);
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.is_empty()); // arrival_rate=0 → 不动作
}

#[test]
fn zi_noise_arrival_rate_one_acts_on_some_stock() {
    let mut s = ZiNoiseStrategy::new(1.0, 100, 0.0, 1).unwrap();
    let mv = one_stock_view(1000, None);
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.3)); // 0.3<0.5 → 买
    assert_eq!(ints.len(), 1);
    assert!(matches!(ints[0], Intent::PlaceLimit { side: Side::Buy, qty: 100, .. }));
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
        stocks.insert(StockCode("600101".to_string()), StockView {
            best_bid: Some(Money::from_cents(999)),
            best_ask: Some(Money::from_cents(1001)),
            last_price: Money::from_cents(1050),
            fundamental_value: None,
            recent_prices: vec![Money::from_cents(1000), Money::from_cents(1020), Money::from_cents(1050)], // 上升
        });
        MarketView { stocks }
    };
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(i, Intent::PlaceLimit { side: Side::Buy, .. })));
}

#[test]
fn zi_noise_rejects_invalid_params() {
    assert!(ZiNoiseStrategy::new(1.5, 100, 0.0, 1).is_err()); // arrival_rate>1
    assert!(ZiNoiseStrategy::new(0.5, 0, 0.0, 1).is_err());   // order_size_mean=0
    assert!(ZiNoiseStrategy::new(0.5, 100, 0.0, 0).is_err()); // tick_cents=0
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `cannot find ZiNoiseStrategy`（RED）。

- [ ] **Step 3: 写实现**

追加到 `packages/engine/src/strategy.rs`：
```rust
use thiserror::Error;

/// 策略构造/参数失败。
#[derive(Debug, Error)]
pub enum StrategyError {
    #[error("invalid param {param}: {reason}")]
    InvalidParam { param: &'static str, reason: String },
}

/// 散户：零智力泊松 + 少量追涨杀跌。
pub struct ZiNoiseStrategy {
    arrival_rate: f64,
    order_size_mean: u32,
    chase_prob: f64,
    tick_cents: i64,
}

impl ZiNoiseStrategy {
    pub fn new(arrival_rate: f64, order_size_mean: u32, chase_prob: f64, tick_cents: i64) -> Result<Self, StrategyError> {
        if !(arrival_rate >= 0.0 && arrival_rate <= 1.0) {
            return Err(StrategyError::InvalidParam { param: "arrival_rate", reason: format!("{arrival_rate} not in [0,1]") });
        }
        if order_size_mean == 0 {
            return Err(StrategyError::InvalidParam { param: "order_size_mean", reason: "must be > 0".to_string() });
        }
        if !(chase_prob >= 0.0 && chase_prob <= 1.0) {
            return Err(StrategyError::InvalidParam { param: "chase_prob", reason: format!("{chase_prob} not in [0,1]") });
        }
        if tick_cents <= 0 {
            return Err(StrategyError::InvalidParam { param: "tick_cents", reason: "must be > 0".to_string() });
        }
        Ok(ZiNoiseStrategy { arrival_rate, order_size_mean, chase_prob, tick_cents })
    }
}

impl Strategy for ZiNoiseStrategy {
    fn decide(&mut self, market: &MarketView, _own: &SelfView, rng: &mut dyn Rng) -> Vec<Intent> {
        if market.stocks.is_empty() || rng.next_f64() >= self.arrival_rate {
            return Vec::new();
        }
        // 选一只股票（取第一个键——确定性便于测试；生产可随机，但 v1 取首键）。
        let (code, sv) = match market.stocks.first_key_value() {
            Some((c, v)) => (c.clone(), v),
            None => return Vec::new(),
        };
        // 追势？
        if rng.next_f64() < self.chase_prob {
            if trend_up(sv) {
                return vec![Intent::PlaceLimit { code, side: Side::Buy, price: sv.last_price, qty: self.order_size_mean }];
            } else if trend_down(sv) {
                return vec![Intent::PlaceLimit { code, side: Side::Sell, price: sv.last_price, qty: self.order_size_mean }];
            }
        }
        // 随机买卖各半：next_f64 < 0.5 → 买，否则卖。
        let side = if rng.next_f64() < 0.5 { Side::Buy } else { Side::Sell };
        let price = match side {
            Side::Buy => sv.best_bid.unwrap_or(sv.last_price).add_cents(self.tick_cents),
            Side::Sell => sv.best_ask.unwrap_or(sv.last_price).sub_cents_or_zero(self.tick_cents),
        };
        vec![Intent::PlaceLimit { code, side, price, qty: self.order_size_mean }]
    }
}

/// recent_prices 末段是否上升（至少 2 个点且末>首）。
fn trend_up(sv: &StockView) -> bool {
    let p = &sv.recent_prices;
    p.len() >= 2 && p.last().unwrap().cents() > p.first().unwrap().cents()
}
fn trend_down(sv: &StockView) -> bool {
    let p = &sv.recent_prices;
    p.len() >= 2 && p.last().unwrap().cents() < p.first().unwrap().cents()
}
```
> 需 `Money` 的 `add_cents`/`sub_cents_or_zero` 辅助——money 模块尚无。**本任务在 strategy.rs 内加 Money 局部辅助 trait** 或直接用 `Money::from_cents(cents ± tick)`。**采用直接算**：
```rust
        let price = match side {
            Side::Buy => Money::from_cents(sv.best_bid.unwrap_or(sv.last_price).cents() + self.tick_cents),
            Side::Sell => Money::from_cents((sv.best_ask.unwrap_or(sv.last_price).cents() - self.tick_cents).max(0)),
        };
```
（用上面这两行替换 `add_cents`/`sub_cents_or_zero` 版本，避免动 money 模块。）

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（arrival=0 空、arrival=1 买、追势买、非法参数 全过）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/strategy.rs packages/engine/tests/strategy.rs
git commit -m "feat(engine): ZiNoiseStrategy 散户ZI泊松+追势策略"
```

---

## Task 3: `ValueStrategy`（机构）

**Files:**
- Modify: `packages/engine/src/strategy.rs`
- Modify: `packages/engine/tests/strategy.rs`

**Interfaces:**
- Produces: `pub enum TargetPolicy { Fixed(Money), TrackV{bias:f64}, DriftUp{rate:f64, base:Money} }`；`pub struct ValueStrategy { policy: TargetPolicy, margin:f64, order_size:u32, ticks:u64 }`；`impl Strategy for ValueStrategy`；`ValueStrategy::new(policy, margin, order_size) -> Result<Self, StrategyError>`（margin∈[0,1)、order_size>0）。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/strategy.rs`：
```rust
use engine::strategy::{ValueStrategy, TargetPolicy};

#[test]
fn value_buys_when_undervalued() {
    // V=1000, target=TrackV{bias:0}→target=1000, margin=0.05→买阈 950。last=900<950 → 买
    let mut s = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, 0.05, 100).unwrap();
    let mv = one_stock_view(900, Some(1000)); // last=900, V=1000
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(i, Intent::PlaceLimit { side: Side::Buy, .. })));
}

#[test]
fn value_no_action_when_in_band() {
    let mut s = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, 0.05, 100).unwrap();
    let mv = one_stock_view(1000, Some(1000)); // last=1000 在 [950,1050] 带内
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() };
    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn value_sells_when_overvalued_and_has_position() {
    let mut s = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, 0.05, 100).unwrap();
    let mv = one_stock_view(1100, Some(1000)); // last=1100>1050 → 卖
    let mut pos = BTreeMap::new();
    pos.insert(StockCode("600101".to_string()), PositionView { qty: 100, sellable_qty: 100, cost_price: Some(Money::from_cents(1000)) });
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: pos };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(i, Intent::PlaceLimit { side: Side::Sell, .. })));
}

#[test]
fn value_no_sell_without_position() {
    let mut s = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, 0.05, 100).unwrap();
    let mv = one_stock_view(1100, Some(1000));
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() }; // 无持仓
    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn value_target_policies_differ() {
    // Fixed(800) vs TrackV{bias:0.1} on V=1000 → 800 vs 1100
    let mut s_fixed = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(800)), 0.01, 100).unwrap();
    let mut s_track = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.1 }, 0.01, 100).unwrap();
    let mv = one_stock_view(900, Some(1000)); // V=1000
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() };
    // Fixed target=800, band [792,808]; last=900 > 808 → 应卖但无持仓 → 无动作
    assert!(s_fixed.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
    // TrackV target=1100, band [1089,1111]; last=900 < 1089 → 买
    let ints = s_track.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(i, Intent::PlaceLimit { side: Side::Buy, .. })));
}

#[test]
fn value_ignores_stocks_without_visible_v() {
    let mut s = ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, 0.05, 100).unwrap();
    let mv = one_stock_view(900, None); // V 不可见
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() };
    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty()); // 无 V 不动作
}

#[test]
fn value_rejects_invalid_params() {
    assert!(ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1000)), -0.1, 100).is_err()); // margin<0
    assert!(ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1000)), 0.05, 0).is_err()); // order_size=0
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `cannot find ValueStrategy`（RED）。

- [ ] **Step 3: 写实现**

追加到 `packages/engine/src/strategy.rs`：
```rust
/// 机构目标价策略（每实例一种，机构看法各异）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum TargetPolicy {
    /// 固定目标价。
    Fixed(Money),
    /// 跟随 V：target = V × (1 + bias)。
    TrackV { bias: f64 },
    /// 认为大致上涨：target = base × (1 + rate × ticks)。
    DriftUp { rate: f64, base: Money },
}

/// 机构：基本面价值，读隐藏 V + 目标价策略。
pub struct ValueStrategy {
    policy: TargetPolicy,
    margin: f64,
    order_size: u32,
    ticks: u64,
}

impl ValueStrategy {
    pub fn new(policy: TargetPolicy, margin: f64, order_size: u32) -> Result<Self, StrategyError> {
        if !(margin >= 0.0 && margin < 1.0) {
            return Err(StrategyError::InvalidParam { param: "margin", reason: format!("{margin} not in [0,1)") });
        }
        if order_size == 0 {
            return Err(StrategyError::InvalidParam { param: "order_size", reason: "must be > 0".to_string() });
        }
        Ok(ValueStrategy { policy, margin, order_size, ticks: 0 })
    }

    /// 依策略算目标价。V 不可见（TrackV）→ 返回 None（该股跳过）。
    fn target(&self, v: Option<Money>) -> Option<f64> {
        match &self.policy {
            TargetPolicy::Fixed(m) => Some(m.cents() as f64),
            TargetPolicy::TrackV { bias } => v.map(|vv| vv.cents() as f64 * (1.0 + bias)),
            TargetPolicy::DriftUp { rate, base } => {
                Some(base.cents() as f64 * (1.0 + rate * self.ticks as f64))
            }
        }
    }
}

impl Strategy for ValueStrategy {
    fn decide(&mut self, market: &MarketView, own: &SelfView, _rng: &mut dyn Rng) -> Vec<Intent> {
        self.ticks += 1;
        let mut out = Vec::new();
        for (code, sv) in &market.stocks {
            let target = match self.target(sv.fundamental_value) {
                Some(t) => t,
                None => continue, // V 不可见 → 跳过
            };
            let last = sv.last_price.cents() as f64;
            let low = target * (1.0 - self.margin);
            let high = target * (1.0 + self.margin);
            if last < low {
                out.push(Intent::PlaceLimit { code: code.clone(), side: Side::Buy, price: sv.last_price, qty: self.order_size });
            } else if last > high {
                let sellable = own.positions.get(code).map(|p| p.sellable_qty).unwrap_or(0);
                if sellable > 0 {
                    let qty = self.order_size.min(sellable);
                    out.push(Intent::PlaceLimit { code: code.clone(), side: Side::Sell, price: sv.last_price, qty });
                }
            }
        }
        out
    }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（买/带内不动作/卖/无持仓不卖/目标价差异/无V忽略/非法参数 全过）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/strategy.rs packages/engine/tests/strategy.rs
git commit -m "feat(engine): ValueStrategy 机构基本面策略(读V+目标价Fixed/TrackV/DriftUp)"
```

---

## Task 4: `MomentumStrategy`（游资）

**Files:**
- Modify: `packages/engine/src/strategy.rs`
- Modify: `packages/engine/tests/strategy.rs`

**Interfaces:**
- Produces: `pub struct MomentumStrategy { lookback:usize, trend_threshold:f64, order_size:u32 }`；`impl Strategy`；`new(lookback, trend_threshold, order_size) -> Result<Self, StrategyError>`（lookback>=2、trend_threshold>=0、order_size>0）。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/strategy.rs`：
```rust
use engine::strategy::MomentumStrategy;

fn stock_with_history(code: &str, hist: Vec<i64>) -> MarketView {
    let mut stocks = BTreeMap::new();
    let last = *hist.last().unwrap_or(&1000);
    stocks.insert(StockCode(code.to_string()), StockView {
        best_bid: Some(Money::from_cents(last - 1)),
        best_ask: Some(Money::from_cents(last + 1)),
        last_price: Money::from_cents(last),
        fundamental_value: None,
        recent_prices: hist.into_iter().map(Money::from_cents).collect(),
    });
    MarketView { stocks }
}

#[test]
fn momentum_buys_on_uptrend() {
    let mut s = MomentumStrategy::new(3, 0.02, 100).unwrap();
    let mv = stock_with_history("600101", vec![1000, 1020, 1050]); // +5% > 2%
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(i, Intent::PlaceLimit { side: Side::Buy, .. })));
}

#[test]
fn momentum_sells_on_downtrend_with_position() {
    let mut s = MomentumStrategy::new(3, 0.02, 100).unwrap();
    let mv = stock_with_history("600101", vec![1050, 1020, 1000]); // -4.76% < -2%
    let mut pos = BTreeMap::new();
    pos.insert(StockCode("600101".to_string()), PositionView { qty: 100, sellable_qty: 100, cost_price: Some(Money::from_cents(1020)) });
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: pos };
    let ints = s.decide(&mv, &own, &mut SeqRng::new_f64(0.5));
    assert!(ints.iter().any(|i| matches!(i, Intent::PlaceLimit { side: Side::Sell, .. })));
}

#[test]
fn momentum_no_action_on_flat() {
    let mut s = MomentumStrategy::new(3, 0.02, 100).unwrap();
    let mv = stock_with_history("600101", vec![1000, 1005, 1003]); // ~+0.3% < 2%
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() };
    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn momentum_no_sell_without_position() {
    let mut s = MomentumStrategy::new(3, 0.02, 100).unwrap();
    let mv = stock_with_history("600101", vec![1050, 1020, 1000]); // 下跌
    let own = SelfView { cash: Money::from_cents(1_000_000), positions: BTreeMap::new() }; // 无持仓
    assert!(s.decide(&mv, &own, &mut SeqRng::new_f64(0.5)).is_empty());
}

#[test]
fn momentum_rejects_invalid_params() {
    assert!(MomentumStrategy::new(1, 0.02, 100).is_err());   // lookback<2
    assert!(MomentumStrategy::new(3, -0.1, 100).is_err());   // threshold<0
    assert!(MomentumStrategy::new(3, 0.02, 0).is_err());     // order_size=0
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `cannot find MomentumStrategy`（RED）。

- [ ] **Step 3: 写实现**

追加到 `packages/engine/src/strategy.rs`：
```rust
/// 游资：短期趋势/动量，快进快出。
pub struct MomentumStrategy {
    lookback: usize,
    trend_threshold: f64,
    order_size: u32,
}

impl MomentumStrategy {
    pub fn new(lookback: usize, trend_threshold: f64, order_size: u32) -> Result<Self, StrategyError> {
        if lookback < 2 {
            return Err(StrategyError::InvalidParam { param: "lookback", reason: "must be >= 2".to_string() });
        }
        if trend_threshold < 0.0 {
            return Err(StrategyError::InvalidParam { param: "trend_threshold", reason: "must be >= 0".to_string() });
        }
        if order_size == 0 {
            return Err(StrategyError::InvalidParam { param: "order_size", reason: "must be > 0".to_string() });
        }
        Ok(MomentumStrategy { lookback, trend_threshold, order_size })
    }
}

impl Strategy for MomentumStrategy {
    fn decide(&mut self, market: &MarketView, own: &SelfView, _rng: &mut dyn Rng) -> Vec<Intent> {
        let mut out = Vec::new();
        for (code, sv) in &market.stocks {
            let p = &sv.recent_prices;
            if p.len() < 2 {
                continue;
            }
            // 取最近 lookback 个点（不足则全取）。
            let start_idx = p.len().saturating_sub(self.lookback);
            let first = p[start_idx].cents() as f64;
            let last = p.last().unwrap().cents() as f64;
            if first <= 0.0 {
                continue;
            }
            let change = (last - first) / first; // 相对变化
            if change > self.trend_threshold {
                out.push(Intent::PlaceLimit { code: code.clone(), side: Side::Buy, price: sv.last_price, qty: self.order_size });
            } else if change < -self.trend_threshold {
                let sellable = own.positions.get(code).map(|p| p.sellable_qty).unwrap_or(0);
                if sellable > 0 {
                    let qty = self.order_size.min(sellable);
                    out.push(Intent::PlaceLimit { code: code.clone(), side: Side::Sell, price: sv.last_price, qty });
                }
            }
        }
        out
    }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（涨买/跌卖/平不动作/无持仓不卖/非法参数 全过）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/strategy.rs packages/engine/tests/strategy.rs
git commit -m "feat(engine): MomentumStrategy 游资趋势动量策略"
```

---

## Task 5: `StrategyFactory` + `StrategyParams`（每实例采样）

**Files:**
- Modify: `packages/engine/src/strategy.rs`
- Modify: `packages/engine/tests/strategy.rs`

**Interfaces:**
- Produces: `pub struct StrategyParams { retail: RetailParams, inst: InstParams, hot: HotParams }`（各含均值/范围）；`pub struct StrategyFactory;`；`StrategyFactory::build(kind: AccountKind, params: &StrategyParams, rng: &mut dyn Rng) -> Option<Box<dyn Strategy>>`（Player→None；采样每实例参数）。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/strategy.rs`：
```rust
use engine::strategy::{StrategyFactory, StrategyParams, RetailParams, InstParams, HotParams};
use engine::account::AccountKind;

fn sample_params() -> StrategyParams {
    StrategyParams {
        retail: RetailParams { arrival_rate: 0.5, order_size_mean: 100, chase_prob: 0.2, tick_cents: 1 },
        inst: InstParams { margin: 0.05, order_size: 200 },
        hot: HotParams { lookback: 3, trend_threshold: 0.02, order_size: 150 },
    }
}

#[test]
fn factory_builds_retail() {
    let p = sample_params();
    let s = StrategyFactory::build(AccountKind::Retail, &p, &mut SeqRng::new_f64(0.5));
    assert!(s.is_some());
}

#[test]
fn factory_player_returns_none() {
    let p = sample_params();
    assert!(StrategyFactory::build(AccountKind::Player, &p, &mut SeqRng::new_f64(0.5)).is_none());
}

#[test]
fn factory_builds_each_kind() {
    let p = sample_params();
    assert!(StrategyFactory::build(AccountKind::Retail, &p, &mut SeqRng::new_f64(0.5)).is_some());
    assert!(StrategyFactory::build(AccountKind::Inst, &p, &mut SeqRng::new_f64(0.5)).is_some());
    assert!(StrategyFactory::build(AccountKind::Hot, &p, &mut SeqRng::new_f64(0.5)).is_some());
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `cannot find StrategyFactory`（RED）。

- [ ] **Step 3: 写实现**

追加到 `packages/engine/src/strategy.rs`（顶部加 `use crate::account::AccountKind;`）：
```rust
/// 散户策略分布参数（每实例从中采样/直接取）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RetailParams {
    pub arrival_rate: f64,
    pub order_size_mean: u32,
    pub chase_prob: f64,
    pub tick_cents: i64,
}
/// 机构策略分布参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct InstParams {
    pub margin: f64,
    pub order_size: u32,
}
/// 游资策略分布参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HotParams {
    pub lookback: usize,
    pub trend_threshold: f64,
    pub order_size: u32,
}
/// 每类 NPC 的策略参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StrategyParams {
    pub retail: RetailParams,
    pub inst: InstParams,
    pub hot: HotParams,
}

/// 策略工厂：按种类构造策略实例（每实例参数从分布/配置取）。
pub struct StrategyFactory;
impl StrategyFactory {
    pub fn build(kind: AccountKind, params: &StrategyParams, _rng: &mut dyn Rng) -> Option<Box<dyn Strategy>> {
        match kind {
            AccountKind::Retail => {
                let r = &params.retail;
                Some(Box::new(ZiNoiseStrategy::new(r.arrival_rate, r.order_size_mean, r.chase_prob, r.tick_cents).ok()?))
            }
            AccountKind::Inst => {
                let i = &params.inst;
                // 机构目标价：v1 用 TrackV{bias:0}（跟随 V）；后续可按实例采样 bias。
                Some(Box::new(ValueStrategy::new(TargetPolicy::TrackV { bias: 0.0 }, i.margin, i.order_size).ok()?))
            }
            AccountKind::Hot => {
                let h = &params.hot;
                Some(Box::new(MomentumStrategy::new(h.lookback, h.trend_threshold, h.order_size).ok()?))
            }
            AccountKind::Player => None,
        }
    }
}
```
> v1 工厂直接取配置参数（每实例暂相同）；「每实例独立采样」的差异化留待 StrategyParams 扩展为分布（均值/方差）后——本批次先打通工厂链路，参数差异化是后续增强（spec §5 已注明分布参数实现期定）。**注意：这意味着 v1 同类 NPC 参数相同；如需差异化，可让 RNG 决定每实例参数微扰——本任务可选加，但先保证工厂可用。**

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（retail Some / player None / 各类 Some）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/strategy.rs packages/engine/tests/strategy.rs
git commit -m "feat(engine): StrategyFactory + StrategyParams 工厂采样"
```

---

## Task 6: 导出 + clippy 清零 + 全量回归

**Files:**
- Modify: `packages/engine/src/lib.rs`
- Modify: `packages/engine/src/strategy.rs`（若 clippy 需）
- Modify: `packages/engine/tests/strategy.rs`（re-export 测试）

- [ ] **Step 1: 写 re-export 测试**

追加到 `packages/engine/tests/strategy.rs`：
```rust
#[test]
fn reexport_from_crate_root() {
    use engine::{ZiNoiseStrategy, ValueStrategy, MomentumStrategy, StrategyFactory, StrategyParams, TargetPolicy, StrategyError, Intent, MarketView, StockView, SelfView, PositionView};
    let _ = ZiNoiseStrategy::new(0.5, 100, 0.1, 1).unwrap();
    let _ = ValueStrategy::new(TargetPolicy::Fixed(Money::from_cents(1000)), 0.05, 100).unwrap();
    let _ = MomentumStrategy::new(3, 0.02, 100).unwrap();
    let _: StrategyParams = sample_params();
    let _: Intent = Intent::PlaceMarket { code: StockCode("x".to_string()), side: engine::Side::Buy, qty: 1 };
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 re-export 缺（RED）。

- [ ] **Step 3: 更新 lib.rs re-export**

`packages/engine/src/lib.rs` 找到现有 `pub use strategy::{Strategy, Intent, MarketView, Rng};`，**扩展为**：
```rust
pub use strategy::{
    Strategy, Intent, MarketView, StockView, SelfView, PositionView, Rng,
    ZiNoiseStrategy, ValueStrategy, MomentumStrategy, TargetPolicy,
    StrategyFactory, StrategyParams, RetailParams, InstParams, HotParams,
    StrategyError,
};
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS。

- [ ] **Step 5: clippy 清零**

Run: `cargo clippy -p engine --all-targets -- -D warnings`
Expected: 零告警。若有（未用 `_own`/`_rng`、`ok()?` 在 Option 上下文、 needless borrow 等）按 clippy 建议修，重跑清零。
> 关注：`StrategyFactory::build` 返回 `Option<Box<dyn Strategy>>` 用 `.ok()?` 在 `Option` 上下文需 `Box::new(... .ok()?)`——确保编译；`_own`/`_rng` 未用参数加下划线前缀。

- [ ] **Step 6: 全量回归**

Run: `cargo test -p engine`
Expected: 全绿（money 20 + config 28 + orderbook 15 + account 18 + market 15 + strategy 新增，应 ≥ 110）。

Run: `cargo build -p engine`
Expected: 无 warning。

- [ ] **Step 7: 提交**

```bash
git add packages/engine/src/lib.rs packages/engine/src/strategy.rs packages/engine/tests/strategy.rs
git commit -m "feat(engine): strategy 导出 + clippy 清零 + 全量回归"
```

---

## Self-Review（plan 作者自检）

**1. Spec 覆盖：**
- §3 视图类型（StockView/MarketView/SelfView/PositionView/Intent+code/decide→Vec）→ Task 1 ✓
- §4 ZiNoise/Value/Momentum 三策略 → Task 2/3/4 ✓
- §5 StrategyFactory + StrategyParams → Task 5 ✓
- §6 错误处理（参数非法 StrategyError、策略不 panic）→ Task 2/3/4/5 ✓
- §8 测试矩阵 10 项 → 散布 Task 1-6 ✓（视图serde/ZI决策/ZI追势/Value买/Value卖/目标价/Momentum趋势/V不可见/工厂/参数非法）

**2. 占位扫描：** 无 TBD/TODO；Task 2 的 `add_cents`/`sub_cents_or_zero` 已明确改为直接 `Money::from_cents(cents ± tick)`。每步含完整可编译代码 ✓。

**3. 类型一致性：**
- `Intent { PlaceLimit{code,side,price,qty}, PlaceMarket{code,side,qty}, Cancel{code,id} }`（无 Pass）跨 Task 1-5 一致 ✓。
- `Strategy::decide(&mut self, &MarketView, &SelfView, &mut dyn Rng) -> Vec<Intent>` 跨任务一致 ✓。
- `StrategyError::InvalidParam{param,reason}` 跨 Task 2/3/4 一致 ✓。
- `TargetPolicy { Fixed(Money), TrackV{bias}, DriftUp{rate,base} }` 跨 Task 3/5 一致 ✓。
- `StrategyFactory::build(AccountKind, &StrategyParams, &mut dyn Rng) -> Option<Box<dyn Strategy>>` Task 5 ✓。
- 复用 `AccountKind`(account)、`StockCode`(account)、`Side`/`OrderId`(orderbook)、`Money`(money)、`Rng`(strategy) ✓。

**已知风险（实现期注意，非 plan 缺陷）：**
- Task 1 演化骨架**破坏 account.rs 旧测试**——Step 1 先更新该测试（适配多股 MarketView + 无 Pass），否则 RED 阶段全 crate 编译失败。务必先改测试。
- Task 2 用 `Money::from_cents(cents ± tick)` 直接算价格（不动 money 模块）；卖价 `.max(0)` 防负。
- Task 5 工厂 v1 同类 NPC 参数相同（spec §5 已注差异化后续）；`.ok()?` 在 `Option` 上下文——`Some(Box::new(ZiNoiseStrategy::new(...).ok()?))` 合法（`?` 在返回 Option 的 fn 内）。
- `ticks` 字段 ValueStrategy 自维护（decide 自增）——DriftUp 用。
