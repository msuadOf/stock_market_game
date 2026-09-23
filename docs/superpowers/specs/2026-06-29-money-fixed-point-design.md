# 设计：engine `money` 模块 —— 定点货币（元×100=分）

- **日期 (Date):** 2026-06-29
- **状态 (Status):** 已批准（设计稿，待 TDD 实现）
- **决策者 (Deciders):** msuad + Claude
- **关联 (Related):** [ADR-0002](../../decisions/0002-engine-rust-wasm.md)（engine 用 Rust）、[ADR-0004](../../decisions/0004-frontend-state-redux-toolkit.md)（engine 持权威状态）

---

## 1. 背景与动机

参考游戏 `ref/模拟股市.html` 全程用 `f64`（JS `Number`）做金额与股价运算，并以 `Number(x.toFixed(2))` 反复取整。
浮点在 `0.1 + 0.2`、反复乘除、跨精度比较中会累积漂移，最终在账目、盈亏、持仓市值上出现「不可解释的 1 分误差」。

engine 作为权威状态源（web/server/desktop 共用同一份 Rust 实现），必须从根上杜绝浮点漂移。
本模块确立 engine 的**钱/价表示地基**，是 engine 首个真实模块，后续所有涉及金额的逻辑（账务、撮合、组合计算、费用）都建立在它之上。

## 2. 核心决策（msuad 2026-06-29 拍板）

1. **存储永远整数定点。** 金额与股价一律存「分」(元×100) 的 `i64`。**绝不**用 `f64` 存权威状态。无舍入误差可言。
2. **价格与资金共用同一 `Money` 类型**（不另开 `Price` 类型，YAGNI）。价格 = 每股的元值，2 位小数，与资金同尺度。
3. **比率用 `f64`。** 佣金率 / 印花税率 / 涨跌幅 `limit` 存为 `f64`，仅在「乘出钱」的那一刻经 `apply_rate` **银行家舍入**（round-half-to-even）到最近整数分，落回 `Money`。这是 money 路径里 `f64` **唯一**允许出现处。
4. **可配置。** 佣金率、印花税率、涨跌幅限制是 `GameConfig` 设置项，**不硬编码**。
5. **显示层才除以 100 + 格式化 2 位小数**（纯渲染，不进权威状态）。

## 3. 类型与 API

```rust
/// 金额/股价的定点表示。内部恒为「分」(元×100) 的 i64，无 f64、无误差。
/// 有符号：盈亏/浮亏可为负。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Money(i64);   // 单位：分
```

**纯整数算术（永不触 f64，全精确）：**
- `Money::from_cents(i64) -> Money` —— 规范构造，零舍入。
- `Money::ZERO` 常量。
- `Money::add` / `Money::sub`（checked → `Result`，溢出报错不回绕）。
- `Money::mul_shares(self, u32 股数) -> Result<Money>`（成交量 × 单价，整数乘法）。
- `Money::cents(&self) -> i64`（只读访问内部值）。

**字符串解析（防御式，非法精度显式暴露）：**
- `Money::from_yuan_str(&str) -> Result<Money>`：`"12.34" → 1234`；`"-0.01" → -1`。
- **`"12.345" → Err`**（超过 2 位小数，不静默截断）。
- 空串 / 非数字 / 多个小数点 → `Err`。
- 负号合法（仅负数金额，股价由市场模块保证非负）。

**f64 唯一合法入口（比率桥接）：**
- `Money::apply_rate(self, rate: f64) -> Result<Money>`：`(self.cents as f64) * rate` → **银行家舍入到最近整数分**。
  - 内部断言 `rate.is_finite()`，NaN/Inf → `Err`（绝不静默）。
  - 仅供佣金（成交额×佣金率）、印花税（成交额×印花税率）、涨跌停价（基准×(1±limit)）使用。

**实现约束：**
- `Add`/`Sub` trait：checked 语义在方法里实现；不 impl 会回绕的 `Add`（避免误用）。或 impl panic-on-overflow 版 + 显式 checked 版（见实现计划权衡）。
- 银行家舍入必须**独立单元测试**，含 0.5 边界（→偶数）与负数方向。

## 4. 不在本模块内（明确边界）

- 佣金率 / 印花税率 / 涨跌幅 `limit` 的具体数值与默认值 → `GameConfig`（设置层）。
- 涨跌停 / 价格 tick 语义 → 市场模块（`money` 只提供「乘率取整」的原子能力）。
- 序列化为前端友好结构（分→元字符串）由前端 / 序列化层负责；本模块 serde 直出 `i64` 分。

## 5. 序列化

- `serde`（`Serialize`/`Deserialize`）把 `Money` 直接序列化为 **`i64` 分（裸数字）**。
- 前端 / 存档拿到的永远是整数；显示时 `cents / 100` + `Intl.NumberFormat` 格式化 2 位小数（渲染层 `toFixed`，不参与权威状态）。
- 序列化 round-trip 必须保真：`1234 → 1234`。

## 6. 错误处理（铁律二：错误显式可见）

- 所有可能失败的操作返回 `Result<_, MoneyError>`（`thiserror` 派生）：
  - `ParseFailed { input, reason }` —— 解析失败（非法精度 / 非数字 / 空串）。
  - `Overflow { op, operand }` —— 整数运算溢出。
  - `InvalidRate { rate }` —— `apply_rate` 收到 NaN/Inf。
- **绝不**静默 fallback、绝不 `unwrap` 业务逻辑、绝不吞错误。

## 7. 测试矩阵（TDD：先红后绿）

`packages/engine/tests/money.rs`（或 `src/money.rs` 内 `#[cfg(test)]`），逐条：

1. **构造/访问：** `from_cents(1234).cents() == 1234`；`ZERO == from_cents(0)`。
2. **解析正确：** `from_yuan_str("12.34")? == 1234`；`"-0.01" == -1`；`"0.1" == 10`；`"100" == 10000`。
3. **解析防御：** `from_yuan_str("12.345") → Err`；`"" → Err`；`"abc" → Err`；`"1.2.3" → Err`；`"--1" → Err`。
4. **纯整数算术：** `add`/`sub` 精确；`mul_shares(100股)` 整数精确；checked 溢出 → `Err`。
5. **`apply_rate` 银行家舍入（round-half-to-even，验证正确性）：**
   - 精确 0.5 分边界 → 取最近偶数：`from_cents(250) * 0.01 == 2.5 分 → apply_rate 结果 = 2 分`；
     `350 * 0.01 == 3.5 → 4 分`；`750 * 0.01 == 7.5 → 8 分`。
   - 典型佣金：`from_cents(1_000_000) * 0.00025`（即 10000.00 元 × 0.00025 = 2.50 元 = 250 分）→ 结果 `250 分`。
   - 非半数（正常四舍）：`from_cents(123) * 0.10 == 12.3 分 → 12 分`。
   - 负数 × 率：`from_cents(-10) * 0.50 == -5 分 → -4 分`（-5 为奇数，向偶数 -4）。
   - NaN → `Err`；+Inf → `Err`。
6. **serde 往返：** 序列化 `1234` → 反序列化回 `Money(1234)`；round-trip 保真。
7. **派生语义：** `Eq/Ord` 正确（比较的是分值）；`Copy` 可用。

> 注：银行家舍入测试数值在实现阶段以**代码实际行为**为准（确保 `round_half_to_even` 实现正确），spec 列举的期望值用于驱动红测试。

## 8. 文件布局

```
packages/engine/src/
├── lib.rs          # 现有 doc-comment；追加 `pub mod money;`
└── money.rs        # Money 类型 + MoneyError + 全部 API
packages/engine/tests/
└── money.rs        # 集成测试（TDD 红绿循环）
```

## 9. 验收标准（Definition of Done）

- `cargo test -p engine` 全绿，覆盖第 7 节测试矩阵全部断言。
- `Money` 内部恒为 `i64` 分，源码中无 `f64` 存储字段（`apply_rate` 入参除外）。
- `MoneyError` 覆盖三类失败；无静默 fallback。
- `lib.rs` 导出 `pub mod money`，`pub use money::Money` 便于外部使用。
- 提交信息遵循 Conventional Commits：`feat(engine): ...` / `test(engine): ...`。
