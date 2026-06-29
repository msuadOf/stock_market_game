# 设计：engine `session`/`simulator` 模块 —— 编排层（把 6 模块串成完整一局）

- **日期 (Date):** 2026-06-29
- **状态 (Status):** 已批准（设计稿，待 TDD 实现）
- **决策者 (Deciders):** msuad + Claude
- **关联 (Related):** [ADR-0005](../../decisions/0005-unified-engine-three-deployments.md)（tick 步进、种子化 RNG、事件流 seq、Session 抽象、联机预留）、[ADR-0006](../../decisions/0006-npc-strategy-module.md)；既有 money/config/orderbook/account/market/strategy 模块。

---

## 1. 背景与动机

engine 已落地 6 个解耦模块：`money`（钱）、`config`（旋钮）、`orderbook`（撮合）、`account`（账务）、`market`（单股价/涨跌停/V）、`strategy`（三策略 + 工厂）。但它们各自独立，缺一个**编排层**把它们串成「每 tick 完整循环」、产出可被前端消费的事件流。`session` 就是这个编排层——ADR-0005 §5 的 `GameSession` 抽象落地。

msuad 2026-06-29 拍板两个关键点：
1. **玩家随时入队、step 才执行**：玩家 UI 下单进队列，下一个 `step()` 开头和所有 NPC 意图一起、按统一顺序处理（公平、可重放、确定性）。
2. **事件流 = 快照 + 增量混合**（参考真实交易软件同花顺/FIX 协议的 Snapshot+Incremental 模式）：`snapshot()` 返回完整状态快照（首次连/断线重连定基线），`step()` 返回带单调 seq 的增量事件（成交/价格跳动/日界/意图被拒）。

## 2. 核心决策

1. **种子化 RNG（SplitMix64）存入 session**：engine 自带确定性 PRNG 实现 `Rng` trait；新局从种子构造，测试注入固定种子可断言精确的 NPC 行为与撮合序列（TDD 可重放）。
2. **`step() -> Vec<Event>` 每 tick 批处理循环**（确定性）：
   a. 收集所有 Intent：按 AccountId 排序遍历 NPC 账户 → 构建视图（机构视图带 V、散户/游资不带；含 recent_prices）+ SelfView → `strategy.decide()` → 收集；加 pending 玩家意图；清空 pending。
   b. **可行性预校验 + 路由**：每个 Intent 先校验（资金/持仓/涨跌停）；不可行 → `IntentRejected` 事件（**不静默丢弃**，呼应 strategy spec §6 + 铁律二）；可行 → 路由 `market.place`。
   c. 结算：每笔成交 → maker/taker 双方 `account.apply_trade`；失败 → `SettlementError` 事件（防御式记录）。
   d. V 演化：每只股票 `evolve_v`；失败 → `VError` 事件（跳过该股，不崩溃）。
   e. 更新价格历史（推入各股 last_price，滚动窗口）。
   f. 日界判定：`tick` 跨天 → `market.end_of_day` + `DayBoundary` 事件。
   g. seq 自增，产出 `PriceTick`（每股）+ 其它事件。
3. **`snapshot() -> Snapshot`**：完整状态快照（所有市场价/盘口/V + 所有账户现金/持仓），带 seq（对应最后事件序号）。存档与前端基线共用。
4. **`enqueue_player_intent(player_id, intent)`**：随时入队；step 开头才消费。玩家账户固定 `AccountId(0)`，NPC 从 1 起。
5. **确定性保证**：NPC 遍历按 BTreeMap 自然序（AccountId 升序）；单一 RNG 源贯穿（NPC 决策、V 演化共用 session.rng，调用顺序固定）→ 同种子同输入同输出。
6. **`new()` 返回 `Result`**（setup 非法 → `SessionError`）；`step()` 返回 `Vec<Event>`（**循环不因单项失败中断**，失败进事件流；致命配置错误在构造期已拦）。
7. **T+0/T1 在 `SessionSetup`**：不动既有 GameConfig；`t1_enabled` 由 setup 传给 `account.apply_trade`。

## 3. 类型与 API

### 3.1 种子化 RNG

```rust
/// SplitMix64：确定性 PRNG（实现 strategy::Rng）。种子化、可重放。
pub struct SplitMix64 { state: u64 }
impl SplitMix64 {
    pub fn new(seed: u64) -> Self;
    pub fn next_u64(&mut self) -> u64; // 标准 SplitMix64 算法
}
impl Rng for SplitMix64 {
    fn next_f64(&mut self) -> f64;          // (next_u64()>>11)/(2^53) → [0,1)
    fn next_range_u32(&mut self, lo: u32, hi: u32) -> u32; // lo + next%range
}
```

### 3.2 事件流（增量 + 快照）

```rust
/// 增量事件。每个带单调 seq（断线续传/对账用）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum Event {
    /// 一笔成交。
    Trade { seq: u64, code: StockCode, price: Money, qty: u32, maker: AccountId, taker: AccountId },
    /// 价格跳动（每股每 tick 末发，含无成交时）。
    PriceTick { seq: u64, code: StockCode, last_price: Money },
    /// 日界（tick 跨天）。
    DayBoundary { seq: u64, day: u32 },
    /// 意图被拒（不可行下单，不静默丢弃）。
    IntentRejected { seq: u64, account: AccountId, code: StockCode, reason: RejectionReason },
    /// 结算失败（防御式：撮合后结算异常，理论上不该，如实记录）。
    SettlementError { seq: u64, account: AccountId, code: StockCode, reason: String },
    /// V 演化失败（防御式：跳过该股）。
    VError { seq: u64, code: StockCode, reason: String },
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum RejectionReason {
    InsufficientCash,  // 买方资金不足（最坏情况预校验）
    InsufficientShares,// 卖方可卖不足
    LimitExceeded,     // 涨跌停超限
    UnknownStock,      // 意图的股票不存在
}

/// 完整状态快照（首次连/断线重连/存档共用）。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    pub seq: u64,
    pub tick: u64,
    pub day: u32,
    pub markets: BTreeMap<StockCode, MarketSnap>,
    pub accounts: BTreeMap<AccountId, AccountSnap>,
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MarketSnap {
    pub last_price: Money, pub last_close: Money,
    pub best_bid: Option<Money>, pub best_ask: Option<Money>,
    pub fundamental_value: Money, // V（含；前端展示层按需过滤，玩家不可见）
}
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct AccountSnap { pub cash: Money, pub positions: BTreeMap<StockCode, PositionSnap> }
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct PositionSnap { pub qty: u32, pub t1_locked: u32, pub invested_cents: i64, pub recovered_cents: i64 }
```

### 3.3 setup + session

```rust
/// 单只股票的初始规格。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct StockSpec {
    pub code: StockCode,
    pub initial_price: Money,
    pub limit_pct: f64,
    pub v_initial: Money,
    pub tick: Money,
}

/// NPC 群体配置。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct NpcSetup {
    pub retail_count: u32,
    pub inst_count: u32,
    pub hot_count: u32,
    pub cash_per_npc: Money,
}

/// session 初始化参数。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct SessionSetup {
    pub stocks: Vec<StockSpec>,
    pub npcs: NpcSetup,
    pub config: GameConfig,        // 交易参数（佣金/印花税等）
    pub v_params: VParams,         // V 演化参数
    pub strategy_params: StrategyParams, // NPC 策略参数
    pub player_cash: Money,        // 玩家初始现金
    pub ticks_per_day: u64,        // 日界：tick % ticks_per_day == 0 触发
    pub history_len: usize,        // 价格历史滚动窗口（供游资趋势）
    pub t1_enabled: bool,          // T+0/T1（传给 account.apply_trade）
}

/// 编排层。持有全部状态；纯逻辑、无 I/O、无全局可变状态（联机预留：每实例即隔离）。
pub struct GameSession {
    setup: SessionSetup,
    rng: SplitMix64,
    markets: BTreeMap<StockCode, Market>,
    accounts: BTreeMap<AccountId, Account>,
    price_history: BTreeMap<StockCode, VecDeque<Money>>,
    pending_player: Vec<Intent>,
    tick: u64,
    day: u32,
    seq: u64,
}

impl GameSession {
    pub fn new(setup: SessionSetup, seed: u64) -> Result<GameSession, SessionError>;
    pub fn enqueue_player_intent(&mut self, player_id: AccountId, intent: Intent) -> Result<(), SessionError>;
    pub fn step(&mut self) -> Vec<Event>;
    pub fn snapshot(&self) -> Snapshot;
    // 只读访问：tick/day/seq/account/market
}
```

### 3.4 错误

```rust
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("invalid setup: {0}")]
    InvalidSetup(String),                 // 股票/NPC/v_params/strategy 非法
    #[error("unknown player: {0:?}")]
    UnknownPlayer(AccountId),             // enqueue 时玩家不存在
    #[error(transparent)] Market(#[from] MarketError),
    #[error(transparent)] Account(#[from] AccountError),
    #[error(transparent)] Money(#[from] MoneyError),
}
```

## 4. step() 流程（确定性批处理）

1. **收集 Intent**：遍历 `accounts`（BTreeMap 自然序 = AccountId 升序）。对每个 NPC（strategy.is_some()）：构建 MarketView（机构 kind=Inst 的 fundamental_value=Some(V)，其它 None；recent_prices 取 price_history 末 history_len）+ SelfView（cash + positions 的 PositionView）→ `strategy.decide(market, own, &mut rng)` → 收集。再加 `pending_player`，清空 pending。
2. **可行性预校验 + 路由**（每个 Intent）：
   - `UnknownStock`（code 不在 markets）→ `IntentRejected{UnknownStock}`。
   - 买：`cost = price × qty`（限价；市价用 best_ask×qty）+ `config.commission(cost)`；`cost+commission > account.cash` → `IntentRejected{InsufficientCash}`。
   - 卖：`qty > account.sellable_qty(code)` → `IntentRejected{InsufficientShares}`。
   - 涨跌停：构造 Order 调 `market.place`，若返回 `LimitExceeded` → `IntentRejected{LimitExceeded}`（market 已不改动）。
   - 可行：`market.place(order)` → MatchResult。
3. **结算**：对每笔 trade，对 maker 与 taker 各 `account.apply_trade(config, side, code, price, qty, t1_enabled)`。side 由该账户在此 trade 是 maker 还是 taker + trade 推断（maker 是被动方）。失败 → `SettlementError`（不静默）。
   > 推断 side：taker 的 side = Intent.side；maker 的 side = 反向。例：Intent 买 → taker 买、maker 卖。
4. **V 演化**：遍历 markets，`market.evolve_v(&v_params, &mut rng)`；失败 → `VError`，跳过。
5. **价格历史**：每只股票推入 `last_price`，超过 history_len 则丢弃最旧。
6. **日界**：`tick` 自增后若 `tick % ticks_per_day == 0`（且 tick>0）→ 各 market `end_of_day` + `DayBoundary{day}` 事件，`day` 自增。
7. **PriceTick**：每股产出 `PriceTick{last_price}`。
8. 所有事件共用 `seq`（每条自增）。

## 5. 错误处理（铁律二：分级）

- **致命（构造期）**：`new` 校验 setup（股票非空、v_params/strategy 合法、counts 合理）→ `SessionError::InvalidSetup`。
- **可记录（运行期，进事件流，不中断循环）**：`IntentRejected` / `SettlementError` / `VError`。**绝不静默丢弃**意图、绝不静默吞结算/V 错误。
- step() 始终返回 `Vec<Event>`，不返回 Result（循环不因单项失败中断；致命问题已在构造期拦）。

## 6. 不在本模块内（明确边界）

- 真实联机（多 session 并发隔离、网络协议）→ 宿主层（本模块纯逻辑，实例即隔离，ADR-0005 §5 联机预留）。
- 玩家 UI / Intent 生成 → 前端。
- 存档读写（LocalStorage/服务器/文件）→ save/宿主层（`snapshot()` 提供可序列化数据基础）。
- 真实交易日的集合竞价阶段差异 → 延后（YAGNI，仅 `end_of_day` 钩子）。
- 订单簿逐笔 delta 事件（OrderBookDelta）→ 延后（v1 事件 = Trade/PriceTick/DayBoundary/IntentRejected/SettlementError/VError）。

## 7. 测试矩阵（TDD：先红后绿）

`packages/engine/tests/session.rs`：
1. **SplitMix64 确定性**：同种子两次构造 → `next_u64` 序列完全相同；`next_f64 ∈ [0,1)`；`next_range_u32(10,20)` 落在 [10,20)。
2. **Session::new 构造**：合法 setup → markets/accounts 数量正确（玩家1 + 各类 NPC×count）、玩家 strategy=None、NPC strategy.is_some()；非法 setup（空股票/非法 v_params）→ `InvalidSetup`。
3. **snapshot() 完整**：含所有 markets 的 last_price/V、所有 accounts 的 cash/positions；seq=0（初始）。
4. **step() 产出事件 + seq 单调**：跑若干 step，所有事件 seq 严格递增、含 PriceTick（每股每 tick）。
5. **确定性**：同种子 + 同 setup 两个 session，跑 N step，事件序列完全相同。
6. **NPC 决策驱动成交**：构造一个机构（资金足）+ 一只低估股票（last < V）→ step 后产生 Trade 事件、账户现金变化、持仓变化。
7. **玩家入队 + step 执行**：`enqueue_player_intent` 后 step → 玩家意图被执行（成交/挂单），事件含该账户。
8. **不可行意图 → IntentRejected**（不静默）：玩家买超出资金 → step 产出 `IntentRejected{InsufficientCash}`。
9. **涨跌停拒单**：玩家下单价超涨跌停 → `IntentRejected{LimitExceeded}`，market 未改。
10. **V 演化在 step 内**：机构可见 V（MarketView.fundamental_value=Some）；跑 step 后 market 的 V 变化（固定种子可断言）。
11. **日界**：`ticks_per_day` 到达 → `DayBoundary` 事件 + `day` 自增 + market.last_close 更新。
12. **价格历史滚动**：跑 history_len+1 步后，price_history 长度不超 history_len。
13. **enqueue 未知玩家** → `UnknownPlayer` Err。

## 8. 文件布局

```
packages/engine/src/
├── lib.rs          # 追加 `pub mod session;` + re-export
└── session.rs      # SplitMix64/Event/Snapshot/SessionSetup/GameSession/SessionError + step/snapshot/enqueue
packages/engine/tests/
└── session.rs      # 集成测试
```

复用：`engine::money::{Money,MoneyError}`、`engine::config::{GameConfig}`、`engine::orderbook::{Order,Trade,Side,OrderId,AccountId,MatchResult}`、`engine::account::{Account,Position,AccountKind,StockCode}`、`engine::market::{Market,VParams,MarketError}`、`engine::strategy::{Strategy,Intent,MarketView,StockView,SelfView,PositionView,Rng,StrategyFactory,StrategyParams}`。

## 9. 验收标准（Definition of Done）

- `cargo test -p engine` 全绿（前 6 模块 117 + session 新增，应 ≥ 125）。
- `cargo clippy -p engine --all-targets -- -D warnings` 零告警。
- `cargo build -p engine` 无 warning。
- 源码审计：确定性（同种子同输出）、无全局可变状态/单例（实例隔离）、不可行意图不静默（IntentRejected）、错误分级正确（致命/可记录）、价格全程 Money、seq 单调。
- `lib.rs` 导出 session 公共类型。
- Conventional Commits，scope `engine`。

## 10. 实现期风险与边界

- **SplitMix64 算法常量**：`0x9E3779B97F4A7C15`（increment）/ `0xBF58476D1CE4E5B9`（gamma1）/ `0x94D049BB133111EB`（gamma2）/ 移位 30,27,31——标准实现，确定性依赖，plan 给全代码。
- **maker/taker side 推断**：Intent 是 taker（主动方）；trade.maker/taker 是 AccountId。结算时 taker 的 side = Intent.side，maker 的 side = 反向。需在路由时记住 Intent.side 关联到产生的 trades。
- **price_history 用 VecDeque**：滚动窗口 O(1) 推入/丢弃；构建 MarketView 时取末 history_len 个转为 Vec。
- **V 可见性**：构建 MarketView 时按账户 kind 决定 StockView.fundamental_value：Inst → Some(market.fundamental_value())，其它 → None。
- **可行性预校验的最坏情况**：买方资金校验用「挂单价×qty + commission」（限价单）；撮合实际成交价可能更低（maker 价），故预校验是保守上界，不会误拒可行单。
- **step 不返回 Result**：所有运行期失败进事件流；致命配置错误在 `new` 拦截。
