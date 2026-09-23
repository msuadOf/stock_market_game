# 设计：engine 初始持仓 —— 流通盘分配（让市场「活」起来）

- **日期 (Date):** 2026-06-29
- **状态 (Status):** 已批准（设计稿，待 TDD 实现）
- **决策者 (Deciders):** msuad + Claude
- **关联 (Related):** [`session` 设计](2026-06-29-session-design.md)、[ADR-0005](../../decisions/0005-unified-engine-three-deployments.md)（统一账户、确定性 RNG）。

---

## 1. 背景与动机

session 编排层落地后，发现一个**真实缺口**：所有账户初始 0 持仓 + 禁止做空 → 没人能卖 → 卖盘恒空 → 买单永远撮合不到 → **首笔成交永不发生**，市场是「死」的（价格不动、持仓永远 0）。撮合引擎本身正确，只是缺初始流动性。

msuad 2026-06-29 定调：**初始持仓**——把每只股票的流通盘在 session 启动时分配给 NPC 持有，让玩家进场时切入一个「筹码已被各参与者瓜分、市场已在运转」的真实场景（贴真实股市某一时刻切入的语义）。

**关键设计意图（msuad）**：初始持仓是「给账户设任意仓位」的**通用能力**，同时服务于：
- **新游戏**：随机分配流通盘（默认全随机，可配三类比例）。
- **加载存档**（精确到天，不精确到分时）：从快照恢复每账户的精确仓位。

故分配逻辑须是「设仓位」的通用方法，新游戏用「随机分配」产生、加载存档用「精确仓位」产生，二者复用同一通路。

## 2. 核心决策

1. **`StockSpec` 增加 `float_shares: u32`**：每只股票的流通盘总量。
2. **新增 `FloatAllocation`**：
   ```rust
   pub enum FloatAllocation {
       Random,                              // 默认：所有 NPC 随机分配（权重随机，筹码守恒）
       ByKind { retail: f64, inst: f64, hot: f64 }, // 三类各占比例（类内随机）
   }
   ```
   默认 `Random`。`ByKind` 比例之和≈1（构造校验；运行时按「有 NPC 的种类」归一化）。
3. **`SessionSetup` 增加 `float_allocation: FloatAllocation`**。
4. **`new()` 自动分配**：若 `float_shares > 0` 且有 NPC，按 `float_allocation` 把每只股票流通盘分配给 NPC；玩家**0 持仓**（新进场）。`float_shares == 0` 时不分配（保留「设精确仓位」给加载存档路径）。
5. **筹码守恒**：所有 NPC 对某股票的持仓之和 == 该股票 `float_shares`（绝不超发/凭空）。
6. **成本价 = 开盘价**：初始持仓按 `initial_price` 记成本（`invested_cents = qty × initial_price.cents()`、`recovered_cents = 0`）→ `cost_price = initial_price`。
7. **初始全可卖**：`t1_locked = 0`（历史持仓，非当日买入）。
8. **确定性**：分配用 session 的种子化 RNG（`SplitMix64`）→ 同种子同分配，可重放、可单测。
9. **`Account` 设仓位的通用方法**：`Account::grant_position(code, qty, cost_price)` —— 设 `invested = qty×cost`、`recovered=0`、`t1_locked=0`。新游戏分配与加载存档（设精确仓位）都走它。

## 3. 分配算法（确定性、筹码守恒）

### Random（默认）
1. 收集所有 NPC 的 AccountId（升序，确定性顺序），`n` 个。
2. 每个 NPC 抽随机权重 `w_i = rng.next_f64()`（`>0` 加 epsilon 防零）。
3. `total_w = Σw_i`。
4. 逐 NPC：`q_i = round(float × w_i / total_w)`（f64 算权重归一，最终落 u32 量），clamp 到剩余量。
5. **最后一个 NPC 拿全部剩余** → `Σq_i == float`（精确守恒）。

### ByKind { retail, inst, hot }
1. 按「有 NPC 的种类」归一化比例（某类 0 个 NPC → 其比例分摊给其余类）。
2. 每类分到 `kind_float = float × 归一化比例`。
3. 类内用 Random 同款方法分给该类 NPC（筹码在类内守恒）。
4. 各类合计 == float。

> f64 仅用于「权重归一/比例」，最终量是 u32；这是**股数分配**，非金额，不违反 money 模块「金额不存 f64」铁律。

## 4. 类型与 API

```rust
// StockSpec 增加：
pub struct StockSpec {
    pub code: StockCode,
    pub initial_price: Money,
    pub limit_pct: f64,
    pub v_initial: Money,
    pub tick: Money,
    pub float_shares: u32,        // 新增：流通盘
}

// 新增：
pub enum FloatAllocation {
    Random,
    ByKind { retail: f64, inst: f64, hot: f64 },
}

// SessionSetup 增加：
pub struct SessionSetup {
    // ... 既有字段 ...
    pub float_allocation: FloatAllocation,  // 新增
}

// Account 增加设仓位方法（通用，新游戏+加载存档共用）：
impl Account {
    /// 设一笔持仓：qty 股、成本价 cost_price（invested=qty×cost、recovered=0、t1_locked=0）。
    pub fn grant_position(&mut self, code: StockCode, qty: u32, cost_price: Money) { ... }
}

// GameSession::new() 内：构造完账户后，若 float_shares>0 且有 NPC，调 seed_float()。
impl GameSession {
    fn seed_float(&mut self) {
        // 遍历每只股票：按 setup.float_allocation 分配 float_shares 给 NPC，
        // 调 account.grant_position(code, qty, initial_price)。
    }
}
```

## 5. 错误处理（铁律二）

- `FloatAllocation::ByKind` 比例非法（负值/非有限）→ 构造期/session 校验 → `SessionError::InvalidSetup`，绝不静默用默认。
- 分配用整数 checked 运算防溢出（`qty × price.cents()` 用 i64 checked）。
- **绝不**超发（`Σq_i == float` 强守恒）、绝不静默截断。
- `float_shares==0` → 不分配（合法，留给加载存档路径），不报错。

## 6. 不在本批次内（明确边界）

- **加载存档的精确仓位恢复**：本批次只实现「新游戏随机分配」+ `grant_position` 通用方法；加载存档（从 Snapshot 设仓位）是后续 save 模块的事，但 `grant_position` 已为其铺路。
- **初始挂单（开盘 orderbook 撒单）**：本批次只给持仓；盘口随 NPC 在 step 中下单逐步填充（玩家进场后头几个 tick 市场转活）。若日后要「开盘即有挂单」，是单独的市场做市商特性。
- **lot 对齐**：持仓不强制 100 整数倍（account 允许任意 qty 卖出；lot 校验在挂单层，本批次不动）。

## 7. 测试矩阵（TDD：先红后绿）

`packages/engine/tests/session.rs` 追加：
1. **Random 筹码守恒**：`float_shares=1_000_000`、1 只股票、几个 NPC → `Σ NPC.qty == 1_000_000`。
2. **玩家 0 持仓**：分配后玩家 positions 为空。
3. **确定性**：同种子两次 new() → 各 NPC 持仓完全相同。
4. **成本价正确**：某 NPC 持仓的 `cost_price == initial_price`、`invested_cents == qty × initial_price.cents()`、`t1_locked == 0`。
5. **float_shares==0 不分配**：`float_shares=0` → 所有账户 0 持仓（兼容加载存档路径）。
6. **ByKind 比例**：`ByKind{retail:0.2,inst:0.5,hot:0.3}` → 三类持仓总量≈2:5:3（容差，因整数取整）。
7. **ByKind 缺类**：某类 0 个 NPC → 其比例分摊给其余类，筹码仍守恒。
8. **市场转活（集成）**：分配后跑若干 step → 出现 `Event::Trade`（NPC 有持仓可卖 → 卖盘有货 → 成交）。**这是「缺口已修」的端到端验证。**
9. **非法比例**：`ByKind` 负值 → `InvalidSetup`。
10. **grant_position 单测**：直接设仓位 → invested/recovered/cost 正确。

## 8. 文件布局

```
packages/engine/src/
├── account.rs   # 修改：grant_position 方法
└── session.rs   # 修改：StockSpec.float_shares、FloatAllocation、SessionSetup 字段、seed_float、new 调用
packages/engine/tests/
├── session.rs   # 追加：分配测试
└── account.rs   # 追加：grant_position 测试
```

## 9. 验收标准（Definition of Done）

- `cargo test -p engine` 全绿（133 + 新增）。
- `cargo clippy -p engine --all-targets -- -D warnings` 零告警。
- 源码审计：筹码守恒（Σ==float）、确定性、玩家 0、成本=开盘价、t1_locked=0、grant_position 通用、无超发、无静默。
- **市场转活**：分配后 step 能产生 Trade（缺口修复的硬证据）。
- Conventional Commits，scope `engine`。

## 10. 实现期风险与边界

- **f64 用于权重归一**：股数分配用 f64 算权重、最终落 u32 量；非金额，合规。最后一个 NPC 拿余量保证精确守恒。
- **大流通盘**：`float` 可达 1e8；`qty × price.cents()` 可达 ~1e13，i64 安全（仍用 checked）。
- **new() 自动 seed**：`float_shares>0` 时 new() 内调 seed_float()；既有 session 测试（new() 后 0 持仓）需把 float_shares 设 0 以保持原语义，或断言改为「有持仓」。**实现期：既有测试若依赖 0 持仓，给其 sample_setup 的 float_shares 设 0。**
- **既有 sample_setup**：需加 `float_shares` 与 `float_allocation` 字段 → 既有测试要更新构造（加字段）。
