# 设计：engine `config` (GameConfig) 模块 —— 可配置游戏参数

- **日期 (Date):** 2026-06-29
- **状态 (Status):** 已批准（设计稿，待 TDD 实现）
- **决策者 (Deciders):** msuad + Claude
- **关联 (Related):** [money 定点货币设计](2026-06-29-money-fixed-point-design.md)（`Money::apply_rate` 是本模块的消费方接口）、[ADR-0002](../../decisions/0002-engine-rust-wasm.md)

---

## 1. 背景与动机

engine 首模块 `money` 已落地（定点 i64 分 + `apply_rate` 银行家舍入）。
`apply_rate` 专为「佣金 / 印花税 / 涨跌幅」等**可配置比率**服务，但持有这些比率、并暴露费用计算的类型尚不存在。

msuad 2026-06-29 拍板：佣金率、印花税率、涨跌幅限制**都是可在设置中配置的**（见 `[[money-representation-policy]]`）。
因此需要一个承载这些旋钮的类型。

**为何是 GameConfig 而非 trade/market/player**：trade/market/player 受 `docs/open-questions.md` 的 **Q8（市场确定性）**、
**Q9（核心玩法循环边界）** 阻塞，未拍板前 AI 不得定方向。`GameConfig` 是**纯数据结构 + 边界校验**，
不固化任何游戏数值、不涉及确定性 RNG、不决定玩法边界，是当前**唯一不受阻塞**的下一模块。

## 2. 核心决策

1. **`GameConfig` 是可序列化的纯配置结构**（serde），承载「可配置旋钮」字段。自身无 I/O、无副作用、无全局状态。
2. **比率字段用 `f64`**（佣金率 / 印花税率 / 涨跌幅 limit）—— 与 msuad「比率用 f64」决策一致；比率仅作 `Money::apply_rate` 的入参，绝不参与存储为金额。
3. **金额类字段用 `Money`**（佣金下限、初始资金）—— 遵循 money 模块的定点铁律。
4. **不固化游戏数值**：参考游戏 `ref/模拟股市.html` 提取的值（0.00025 / 0.0005 / 5 元 / 100 股 / 0.10 / 0.05 / 100000）仅由 `proposed_defaults()` 返回，标注「待 msuad 确认」的提议默认，**不作为硬编码常量散落代码**。
5. **构造即校验（fail loud）**：config 是启动期外部输入，非法 → `Result` 显式报错，**绝不静默 fallback**（遵循 `docs/error-handling.md` §5）。校验只强制数学/逻辑底线不变量（非负、有限、`0 < limit < 1`、`lot_size > 0`、`cash ≥ 0`），不涉及玩法平衡。

## 3. 类型与 API

```rust
/// 可配置的游戏参数集合。纯数据 + 边界校验，可序列化。
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GameConfig {
    pub commission_rate: f64,   // 成交额佣金率（ref 提议: 0.00025）
    pub commission_min: Money,  // 佣金下限（ref 提议: 5.00 元 = 500 分）
    pub stamp_tax_rate: f64,    // 印花税率，仅卖（ref 提议: 0.0005）
    pub default_limit: f64,     // 默认涨跌幅（ref 提议: 0.10）
    pub st_limit: f64,          // ST 涨跌幅（ref 提议: 0.05）
    pub lot_size: u32,          // 一手股数（ref 提议: 100）
    pub starting_cash: Money,   // 初始资金（ref 提议: 100000.00 元）
}
```

**`ConfigError`（thiserror）：**
```rust
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// 比率非有限（NaN/Inf）或为负：commission_rate / stamp_tax_rate。
    #[error("invalid rate field {field:?}: value {rate} ({reason})")]
    InvalidRate { field: &'static str, rate: f64, reason: String },
    /// 涨跌幅非法：<= 0 或 >= 1（无意义）。default_limit / st_limit。
    #[error("invalid limit field {field:?}: value {limit} (must be in (0,1))")]
    InvalidLimit { field: &'static str, limit: f64 },
    /// lot_size == 0。
    #[error("invalid lot_size: {0} (must be >= 1)")]
    InvalidLotSize(u32),
    /// starting_cash 为负。
    #[error("invalid starting_cash: {0:?} (must be >= 0)")]
    InvalidCash(Money),
}
```

**方法：**
- `GameConfig::new(commission_rate, commission_min, stamp_tax_rate, default_limit, st_limit, lot_size, starting_cash) -> Result<GameConfig, ConfigError>` —— 逐字段校验，任一非法即 `Err`。
- `GameConfig::proposed_defaults() -> GameConfig` —— 返回 ref 提议默认值（**doc 注释标注「待 msuad 确认」**），内部经 `new()` 构造（提议值本身合法，故 unwrap 安全，但用 `expect` 带说明而非裸 unwrap）。
- `GameConfig::commission(&self, amount: Money) -> Result<Money, MoneyError>` —— `amount.apply_rate(self.commission_rate)` 后与 `commission_min` 取 `max`（ref: 小额佣金触发 5 元下限）。
- `GameConfig::stamp_tax(&self, amount: Money) -> Result<Money, MoneyError>` —— `amount.apply_rate(self.stamp_tax_rate)`，无下限（ref: 印花税无 floor）。

## 4. 不在本模块内（明确边界）

- trade / market / player 逻辑（受 Q8/Q9 阻塞）—— 本模块只提供费用计算**原子能力**与配置载体。
- 价格波动模型、确定性 RNG、撮合、持仓账务 —— 待 Q8/Q9 拍板后单独 ADR + 模块。
- UI 侧「设置面板」—— 前端层职责；本模块只定义 schema + 校验。

## 5. 错误处理（铁律二）

- 所有校验失败返回 `Result<_, ConfigError>`，错误携带 `field` / 实际值 / 原因。
- `proposed_defaults()` 是内部受控构造，提议值恒合法 → 用 `expect("proposed defaults are valid")`，非业务路径吞错。
- 业务方法 `commission`/`stamp_tax` 透传 `apply_rate` 的 `MoneyError`（NaN/Inf 比率已在 `new()` 校验挡掉，运行期不会触发，但类型上仍 `Result` 以防配置被绕过构造）。
- **绝不**静默 fallback（如非法比率 → 默认 0）。

## 6. 测试矩阵（TDD：先红后绿）

`packages/engine/tests/config.rs`，逐条：

1. **ConfigError 构造 + Display**（T1）：四变体可构造、`to_string` 含字段名/值。
2. **GameConfig serde 往返**（T2）：全字段 round-trip 保真（含 Money 字段序列化为裸 i64）。
3. **`new()` 校验**（T3）：
   - 合法：全合法字段 → `Ok`。
   - 非法比率：`commission_rate = -0.1 / NaN / +Inf` → `Err(InvalidRate)`；`stamp_tax_rate` 同。
   - 非法 limit：`default_limit = 0 / 1.0 / -0.1 / 1.5` → `Err(InvalidLimit)`；`st_limit` 同。
   - `lot_size = 0` → `Err(InvalidLotSize)`。
   - `starting_cash < 0` → `Err(InvalidCash)`。
4. **`proposed_defaults()`**（T4）：返回 ref 提议值——`commission_rate==0.00025`、`stamp_tax_rate==0.0005`、`commission_min==from_cents(500)`、`lot_size==100`、`default_limit==0.10`、`st_limit==0.05`、`starting_cash==from_cents(10_000_000)`。
5. **`commission(amount)`**（T5）：
   - 小额触发 floor：小额成交额 × 0.00025 < 5 元 → 结果 == 500 分（commission_min）。
   - 大额走比率：1_000_000 分（10000 元）× 0.00025 = 250 分 → 结果 == 250 分。
6. **`stamp_tax(amount)`**（T6）：1_000_000 分 × 0.0005 = 500 分 → 500 分（无 floor；小额亦无 floor）。
7. **派生/边界**：`Clone`/`Debug` 可用；`proposed_defaults()` 两次调用相等。

## 7. 文件布局

```
packages/engine/src/
├── lib.rs          # 追加 `pub mod config;` + `pub use config::{GameConfig, ConfigError};`
└── config.rs       # GameConfig + ConfigError + new + proposed_defaults + commission + stamp_tax
packages/engine/tests/
└── config.rs       # 集成测试
```

## 8. 验收标准（Definition of Done）

- `cargo test -p engine` 全绿（money 20 + config 新增，预计 ≥ 28）。
- `cargo clippy -p engine --all-targets -- -D warnings` 零告警。
- `cargo build -p engine` 无 warning。
- 源码审计：`GameConfig` 无静默 fallback；比率字段仅作 `apply_rate` 入参；金额字段为 `Money`。
- `lib.rs` 导出 `pub use config::{GameConfig, ConfigError}`。
- Conventional Commits，scope `engine`。
