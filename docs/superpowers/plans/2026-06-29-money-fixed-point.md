# engine `money` 定点货币模块 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 engine 首个真实模块 —— 定点货币类型 `Money`（i64 分，元×100），永不存 f64，比率经银行家舍入桥接，全防御式错误处理。

**Architecture:** `Money` 是不透明 newtype，内部 `i64` 分。纯整数算术精确；`f64` 仅在 `apply_rate` 入口出现并立即银行家舍入回整数分。所有失败返回 `Result<_, MoneyError>`。serde 直出 i64 分。

**Tech Stack:** Rust 2021（`edition.workspace = true`），`serde`、`serde_json`、`thiserror`（workspace 依赖已就位）。测试用 `cargo test -p engine`。

## Global Constraints

- **铁律二（防御式）：** 任何失败必须 `Result` 返回 + 显式 `MoneyError` 变体；**禁止**静默 fallback、禁止 `unwrap`/`expect` 在业务路径、禁止吞错误。
- **铁律一（TDD）：** 每个任务严格 红→绿→提交；先写失败测试再写实现。
- `Money` 内部恒为 `i64` 分；源码中**不得**用 `f64` 字段存储金额（`apply_rate` 的 `rate: f64` 是唯一合法 f64，且为入参非存储）。
- 依赖只许用 workspace 已声明的 `serde`/`serde_json`/`thiserror`；**不引入新依赖**。
- 提交信息 Conventional Commits，scope `engine`：`test(engine):` / `feat(engine):` / `refactor(engine):`。
- 命名与注释风格匹配 `packages/engine/src/lib.rs` 既有中文 doc-comment 风格。

## File Structure

```
packages/engine/src/
├── lib.rs        # 修改：追加 `pub mod money;` 与 `pub use money::{Money, MoneyError};`
└── money.rs      # 新建：Money newtype + MoneyError + 全部 API（纯逻辑）
packages/engine/tests/
└── money.rs      # 新建：集成测试（TDD 红绿驱动；banker's rounding 边界用例）
```

`money.rs`（src）与 `tests/money.rs` 分离：src 内不放 `#[cfg(test)]` 巨型块，集成测试在外部 crate 验证公开 API（更像真实调用方视角，符合 spec 第 7 节）。

---

## Task 1: `MoneyError` 错误类型（先建错误地基）

**Files:**
- Create: `packages/engine/src/money.rs`
- Modify: `packages/engine/src/lib.rs`
- Test: `packages/engine/tests/money.rs`

**Interfaces:**
- Produces: `pub enum MoneyError { ParseFailed { input: String, reason: String }, Overflow { op: &'static str, operand: String }, InvalidRate { rate: f64 } }`（`thiserror::Error`，每个变体 `#[error("...")]`）。

- [ ] **Step 1: 写失败测试 —— MoneyError 可构造且 thiserror 派生 Display 非 trait-impl 报错**

`packages/engine/tests/money.rs`：
```rust
//! engine money 模块集成测试（TDD 红绿循环）。
use engine::money::MoneyError;

#[test]
fn money_error_variants_construct_and_display() {
    let e1 = MoneyError::ParseFailed { input: "12.345".to_string(), reason: "too many digits".to_string() };
    assert_eq!(e1.to_string().contains("12.345"), true);

    let e2 = MoneyError::Overflow { op: "add", operand: "i64 max".to_string() };
    assert_eq!(e2.to_string().contains("add"), true);

    let e3 = MoneyError::InvalidRate { rate: f64::NAN };
    assert_eq!(e3.to_string().contains("NaN"), true);
}
```

`packages/engine/src/lib.rs` 追加（在文件末尾）：
```rust
pub mod money;
pub use money::{Money, MoneyError};
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `unresolved module money` / `cannot find type MoneyError`（RED）。

- [ ] **Step 3: 写最小实现 —— money.rs 仅含 MoneyError**

`packages/engine/src/money.rs`：
```rust
//! 定点货币表示：金额与股价统一存「分」(元×100) 的 i64。
//!
//! 设计见 docs/superpowers/specs/2026-06-29-money-fixed-point-design.md。
//! 铁律：内部永不存 f64；f64 仅作为 apply_rate 的比率入参，立即银行家舍入回整数分。

use thiserror::Error;

/// money 操作失败。绝不静默吞掉（铁律二）。
#[derive(Debug, Error)]
pub enum MoneyError {
    /// 字符串解析失败：非法精度（超过 2 位小数）/ 非数字 / 空串 / 多个小数点。
    #[error("parse failed: input {input:?}: {reason}")]
    ParseFailed { input: String, reason: String },

    /// 整数运算溢出（i64 分，理论上游戏不会触达，但按防御式原则显式暴露）。
    #[error("overflow in {op} with operand {operand}")]
    Overflow { op: &'static str, operand: String },

    /// apply_rate 收到非有限比率（NaN / +Inf / -Inf）。
    #[error("invalid rate: {rate}")]
    InvalidRate { rate: f64 },
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（编译通过 + 1 test passed）。注意：`Money` 尚未定义，`lib.rs` 的 `pub use money::{Money, MoneyError}` 会编译失败 → **本任务 Step 3 也要给 Money 留一个占位**。

**修正 Step 3 实现**（避免 Step 4 因 `Money` 未定义而失败）：在 `money.rs` 末尾追加：
```rust
/// 占位：Task 2 实现真实 Money。本行仅让 lib.rs 的 pub use 编译通过。
#[derive(Debug)]
pub struct Money(i64);
```
Run: `cargo test -p engine` → Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/money.rs packages/engine/src/lib.rs packages/engine/tests/money.rs
git commit -m "test(engine): 建立 MoneyError 错误类型与 money 模块骨架

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 2: `Money` newtype —— 构造、访问、ZERO、派生 trait

**Files:**
- Modify: `packages/engine/src/money.rs`
- Modify: `packages/engine/tests/money.rs`

**Interfaces:**
- Produces: `Money(i64)` newtype，`#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]`；`pub const ZERO: Money`；`pub fn from_cents(cents: i64) -> Money`；`pub fn cents(&self) -> i64`。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/money.rs`：
```rust
use engine::money::Money;

#[test]
fn money_from_cents_and_zero() {
    let m = Money::from_cents(1234);
    assert_eq!(m.cents(), 1234);
    assert_eq!(Money::ZERO.cents(), 0);
}

#[test]
fn money_equality_and_ordering() {
    assert_eq!(Money::from_cents(100), Money::from_cents(100));
    assert!(Money::from_cents(100) < Money::from_cents(200));
}

#[test]
fn money_is_copy() {
    let a = Money::from_cents(50);
    let b = a; // copy
    assert_eq!(a.cents(), 50); // a 仍可用
    assert_eq!(b.cents(), 50);
}

#[test]
fn money_supports_negative() {
    assert_eq!(Money::from_cents(-1).cents(), -1);
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `no function named from_cents` / `no associated item ZERO`（RED）。

- [ ] **Step 3: 写实现**

替换 `packages/engine/src/money.rs` 末尾占位 struct 为：
```rust
/// 金额/股价的定点表示。内部恒为「分」(元×100) 的 i64，无 f64、无误差。
/// 有符号：盈亏/浮亏可为负。价格 = 每股元值，2 位小数，与资金同尺度。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct Money(i64);

impl Money {
    /// 零金额。
    pub const ZERO: Money = Money(0);

    /// 规范构造：直接由「分」构造，零舍入。
    pub fn from_cents(cents: i64) -> Money {
        Money(cents)
    }

    /// 只读访问内部「分」值。
    pub fn cents(&self) -> i64 {
        self.0
    }
}
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（全部 test passed）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/money.rs packages/engine/tests/money.rs
git commit -m "feat(engine): Money newtype(定点 i64 分) 构造/访问/派生 trait

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 3: 纯整数算术 —— `add` / `sub` / `mul_shares`（checked 溢出 → Result）

**Files:**
- Modify: `packages/engine/src/money.rs`
- Modify: `packages/engine/tests/money.rs`

**Interfaces:**
- Produces: `pub fn add(self, other: Money) -> Result<Money, MoneyError>`；`pub fn sub(self, other: Money) -> Result<Money, MoneyError>`；`pub fn mul_shares(self, shares: u32) -> Result<Money, MoneyError>`。全部 checked，溢出返回 `MoneyError::Overflow`。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/money.rs`：
```rust
#[test]
fn money_add_sub_exact() {
    let a = Money::from_cents(100);
    let b = Money::from_cents(25);
    assert_eq!(a.add(b)?.cents(), 125);
    assert_eq!(a.sub(b)?.cents(), 75);
    // 负数
    assert_eq!(Money::from_cents(10).sub(Money::from_cents(30))?.cents(), -20);
    Ok::<(), engine::money::MoneyError>(())
}
```

> 注意：上例用 `?` 在测试函数返回 `Result`。测试函数签名需为 `fn ...() -> Result<(), MoneyError>`。下方完整版：
```rust
use engine::money::MoneyError;

#[test]
fn money_add_sub_exact() -> Result<(), MoneyError> {
    let a = Money::from_cents(100);
    let b = Money::from_cents(25);
    assert_eq!(a.add(b)?.cents(), 125);
    assert_eq!(a.sub(b)?.cents(), 75);
    assert_eq!(Money::from_cents(10).sub(Money::from_cents(30))?.cents(), -20);
    Ok(())
}

#[test]
fn money_mul_shares_exact() -> Result<(), MoneyError> {
    // 单价 12.34 元 × 100 股 = 1234.00 元 = 123400 分
    let price = Money::from_cents(1234);
    assert_eq!(price.mul_shares(100)?.cents(), 123_400);
    // 0 股
    assert_eq!(price.mul_shares(0)?.cents(), 0);
    Ok(())
}

#[test]
fn money_add_overflow_returns_err() {
    let maxed = Money::from_cents(i64::MAX);
    let err = maxed.add(Money::from_cents(1)).unwrap_err();
    assert!(matches!(err, MoneyError::Overflow { op: "add", .. }));
}

#[test]
fn money_mul_shares_overflow_returns_err() {
    // i64::MAX 分 × 2 股必溢出
    let err = Money::from_cents(i64::MAX).mul_shares(2).unwrap_err();
    assert!(matches!(err, MoneyError::Overflow { op: "mul_shares", .. }));
}
```
（删除第一个无 `Result` 签名的草稿版本，保留带签名的。）

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `no method add`（RED）。

- [ ] **Step 3: 写实现**

在 `impl Money { ... }` 块内（Task 2 的 cents() 之后）追加：
```rust
    /// 定点加法（checked，溢出 → Err）。
    pub fn add(self, other: Money) -> Result<Money, MoneyError> {
        self.0
            .checked_add(other.0)
            .map(Money)
            .ok_or_else(|| MoneyError::Overflow {
                op: "add",
                operand: format!("{} + {}", self.0, other.0),
            })
    }

    /// 定点减法（checked，溢出 → Err）。
    pub fn sub(self, other: Money) -> Result<Money, MoneyError> {
        self.0
            .checked_sub(other.0)
            .map(Money)
            .ok_or_else(|| MoneyError::Overflow {
                op: "sub",
                operand: format!("{} - {}", self.0, other.0),
            })
    }

    /// 乘以整数股数（checked，溢出 → Err）。纯整数，无 f64。
    pub fn mul_shares(self, shares: u32) -> Result<Money, MoneyError> {
        self.0
            .checked_mul(i64::try_from(shares).unwrap_or(i64::MAX))
            .map(Money)
            .ok_or_else(|| MoneyError::Overflow {
                op: "mul_shares",
                operand: format!("{} * {}", self.0, shares),
            })
    }
```
> `try_from(shares).unwrap_or(i64::MAX)`：u32→i64 不会失败（u32 max ≪ i64 max），unwrap_or 仅满足类型；若真到 i64::MAX 也必在 checked_mul 溢出报错，符合防御式。

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/money.rs packages/engine/tests/money.rs
git commit -m "feat(engine): Money checked 纯整数算术(add/sub/mul_shares)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 4: 字符串解析 `from_yuan_str`（防御式，非法精度 → Err）

**Files:**
- Modify: `packages/engine/src/money.rs`
- Modify: `packages/engine/tests/money.rs`

**Interfaces:**
- Produces: `pub fn from_yuan_str(s: &str) -> Result<Money, MoneyError>`。
  - `"12.34" → 1234`；`"-0.01" → -1`；`"0.1" → 10`；`"100" → 10000`；`"12." → 1200`（合法，尾随点视作 0 分）；`".5" → 50`。
  - 超过 2 位小数 / 空串 / 非数字 / 多个小数点 / 多个负号 → `Err`。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/money.rs`：
```rust
#[test]
fn money_from_yuan_str_valid() -> Result<(), MoneyError> {
    assert_eq!(Money::from_yuan_str("12.34")?.cents(), 1234);
    assert_eq!(Money::from_yuan_str("-0.01")?.cents(), -1);
    assert_eq!(Money::from_yuan_str("0.1")?.cents(), 10);
    assert_eq!(Money::from_yuan_str("100")?.cents(), 10000);
    assert_eq!(Money::from_yuan_str("12.")?.cents(), 1200);
    assert_eq!(Money::from_yuan_str(".5")?.cents(), 50);
    assert_eq!(Money::from_yuan_str("+3.50")?.cents(), 350);
    Ok(())
}

#[test]
fn money_from_yuan_str_invalid() {
    assert!(matches!(Money::from_yuan_str("12.345"), Err(MoneyError::ParseFailed { .. }))); // 超过 2 位
    assert!(matches!(Money::from_yuan_str(""), Err(MoneyError::ParseFailed { .. })));         // 空
    assert!(matches!(Money::from_yuan_str("abc"), Err(MoneyError::ParseFailed { .. })));      // 非数字
    assert!(matches!(Money::from_yuan_str("1.2.3"), Err(MoneyError::ParseFailed { .. })));    // 多点
    assert!(matches!(Money::from_yuan_str("--1"), Err(MoneyError::ParseFailed { .. })));      // 多负号
    assert!(matches!(Money::from_yuan_str("12.3a"), Err(MoneyError::ParseFailed { .. })));    // 尾部非数字
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `no method from_yuan_str`（RED）。

- [ ] **Step 3: 写实现**

在 `impl Money` 内追加：
```rust
    /// 由「元」字符串解析为分（元×100）。精确到 2 位小数。
    ///
    /// 防御式：超过 2 位小数 / 空串 / 非数字 / 多个小数点 → Err，绝不静默截断。
    pub fn from_yuan_str(s: &str) -> Result<Money, MoneyError> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Err(MoneyError::ParseFailed {
                input: s.to_string(),
                reason: "empty input".to_string(),
            });
        }

        // 拆符号
        let (neg, digits) = match trimmed.as_bytes()[0] {
            b'-' => (true, &trimmed[1..]),
            b'+' => (false, &trimmed[1..]),
            _ => (false, trimmed),
        };
        if digits.is_empty() {
            return Err(MoneyError::ParseFailed {
                input: s.to_string(),
                reason: "sign without digits".to_string(),
            });
        }

        // 拆小数点：最多一个
        let (int_part, frac_part) = match digits.split_once('.') {
            Some((i, f)) => {
                if f.contains('.') {
                    return Err(MoneyError::ParseFailed {
                        input: s.to_string(),
                        reason: "multiple decimal points".to_string(),
                    });
                }
                (i, f)
            }
            None => (digits, ""),
        };

        // 小数部分最多 2 位
        if frac_part.len() > 2 {
            return Err(MoneyError::ParseFailed {
                input: s.to_string(),
                reason: format!("too many fractional digits: {}", frac_part.len()),
            });
        }

        // 整数部分必须全数字（允许空，如 ".5"）
        if !int_part.is_empty() && !int_part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(MoneyError::ParseFailed {
                input: s.to_string(),
                reason: "non-digit in integer part".to_string(),
            });
        }
        // 小数部分必须全数字
        if !frac_part.is_empty() && !frac_part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(MoneyError::ParseFailed {
                input: s.to_string(),
                reason: "non-digit in fractional part".to_string(),
            });
        }

        // 拼成「分」：整数部分 ×100 + 小数部分补零到 2 位
        let int_cents: i64 = if int_part.is_empty() {
            0
        } else {
            int_part.parse::<i64>().map_err(|_| MoneyError::ParseFailed {
                input: s.to_string(),
                reason: "integer part out of range".to_string(),
            })?
        };
        let frac_padded = match frac_part.len() {
            0 => 0i64,
            1 => frac_part.parse::<i64>().unwrap() * 10, // 1 位 → ×10
            2 => frac_part.parse::<i64>().unwrap(),       // 2 位 → 原样
            _ => unreachable!("guarded above"),
        };

        let total = int_cents
            .checked_mul(100)
            .and_then(|c| c.checked_add(if neg { -frac_padded } else { frac_padded }))
            .ok_or_else(|| MoneyError::Overflow {
                op: "from_yuan_str",
                operand: s.to_string(),
            })?;
        Ok(Money(total))
    }
```

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/money.rs packages/engine/tests/money.rs
git commit -m "feat(engine): Money from_yuan_str 防御式解析(非法精度显式报错)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 5: 银行家舍入核心 + `apply_rate`（f64 唯一入口）

**Files:**
- Modify: `packages/engine/src/money.rs`
- Modify: `packages/engine/tests/money.rs`

**Interfaces:**
- Produces: `pub(crate) fn round_half_to_even(x: f64) -> i64`（私有辅助）；`pub fn apply_rate(self, rate: f64) -> Result<Money, MoneyError>`。
  - `apply_rate`：`rate.is_finite()` 否 → `Err(InvalidRate)`；否则 `(self.0 as f64) * rate` 经 `round_half_to_even` 取整为分。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/money.rs`：
```rust
#[test]
fn apply_rate_round_half_to_even_half_boundaries() -> Result<(), MoneyError> {
    // 0.5 边界 → 最近偶数（round-half-to-even）
    // 250 分 × 0.01 = 2.5 → 2 分（偶数）
    assert_eq!(Money::from_cents(250).apply_rate(0.01)?.cents(), 2);
    // 350 分 × 0.01 = 3.5 → 4 分（偶数）
    assert_eq!(Money::from_cents(350).apply_rate(0.01)?.cents(), 4);
    // 750 分 × 0.01 = 7.5 → 8 分（偶数）
    assert_eq!(Money::from_cents(750).apply_rate(0.01)?.cents(), 8);
    // 150 分 × 0.01 = 1.5 → 2 分（偶数）
    assert_eq!(Money::from_cents(150).apply_rate(0.01)?.cents(), 2);
    Ok(())
}

#[test]
fn apply_rate_non_half_normal_rounding() -> Result<(), MoneyError> {
    // 12.3 → 12（正常四舍，<0.5）
    assert_eq!(Money::from_cents(123).apply_rate(0.10)?.cents(), 12);
    // 12.8 → 13（>0.5 进位）
    assert_eq!(Money::from_cents(128).apply_rate(0.10)?.cents(), 13);
    Ok(())
}

#[test]
fn apply_rate_typical_commission() -> Result<(), MoneyError> {
    // 成交额 10000.00 元(=1_000_000 分) × 0.00025 = 2.50 元 = 250 分
    // 2.50 在「分」尺度即整数 250，无半边界争议
    assert_eq!(Money::from_cents(1_000_000).apply_rate(0.00025)?.cents(), 250);
    Ok(())
}

#[test]
fn apply_rate_negative_toward_even() -> Result<(), MoneyError> {
    // -10 分 × 0.50 = -5.0 → -5 为奇数？-5.0 精确，取 -5。
    // 关键：-5 恰为整数，无半边界；验证负数方向不翻转
    assert_eq!(Money::from_cents(-10).apply_rate(0.50)?.cents(), -5);
    // -50 分 × 0.01 = -0.5 → 0（向偶数 0，非 -1）
    assert_eq!(Money::from_cents(-50).apply_rate(0.01)?.cents(), 0);
    Ok(())
}

#[test]
fn apply_rate_nan_inf_rejected() {
    assert!(matches!(
        Money::from_cents(100).apply_rate(f64::NAN),
        Err(MoneyError::InvalidRate { .. })
    ));
    assert!(matches!(
        Money::from_cents(100).apply_rate(f64::INFINITY),
        Err(MoneyError::InvalidRate { .. })
    ));
    assert!(matches!(
        Money::from_cents(100).apply_rate(f64::NEG_INFINITY),
        Err(MoneyError::InvalidRate { .. })
    ));
}

#[test]
fn apply_rate_zero_rate() -> Result<(), MoneyError> {
    assert_eq!(Money::from_cents(12345).apply_rate(0.0)?.cents(), 0);
    Ok(())
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: 编译错误 `no method apply_rate`（RED）。

- [ ] **Step 3: 写实现**

在 `packages/engine/src/money.rs` 末尾（`impl Money` 之外或之内皆可，放 `impl Money` 内）追加：
```rust
    /// f64 唯一合法入口：按比率(佣金率/印花税率/涨跌幅 limit)缩放金额，
    /// 银行家舍入(round-half-to-even)到最近整数分。比率必须有限。
    ///
    /// 这是 money 路径里 f64 唯一允许出现处；结果立即落回整数 Money。
    pub fn apply_rate(self, rate: f64) -> Result<Money, MoneyError> {
        if !rate.is_finite() {
            return Err(MoneyError::InvalidRate { rate });
        }
        let scaled = (self.0 as f64) * rate;
        Ok(Money(round_half_to_even(scaled)))
    }
```

并在 `impl Money` **之外**（模块级私有函数）追加：
```rust
/// 银行家舍入（round-half-to-even）：0.5 向最近偶数；其余正常四舍五入。
/// 负数对称：-0.5 → 0，-1.5 → -2。
fn round_half_to_even(x: f64) -> i64 {
    // 利用 libc::rint? 不引入依赖。手写：
    // 取 floor 与 0.5 比较
    let floor = x.floor();
    let diff = x - floor;
    match diff {
        d if d < 0.5 => floor as i64,
        d if d > 0.5 => (floor + 1.0) as i64,
        // 恰为 0.5：看 floor 奇偶。floor 为偶 → 取 floor；奇 → 取 floor+1。
        _ => {
            if (floor as i64) % 2 == 0 {
                floor as i64
            } else {
                (floor + 1.0) as i64
            }
        }
    }
}
```
> 注意负数：`x=-0.5` → `floor=-1.0`, `diff=0.5` → floor 为奇(-1) → 取 floor+1=0 ✓。
> `x=-1.5` → `floor=-2.0`, `diff=0.5` → floor 为偶(-2) → 取 -2 ✓。

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（银行家舍入全部边界用例通过）。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/money.rs packages/engine/tests/money.rs
git commit -m "feat(engine): Money apply_rate 银行家舍入(f64 唯一桥接入口)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 6: serde 序列化（直出 i64 分，往返保真）

**Files:**
- Modify: `packages/engine/src/money.rs`
- Modify: `packages/engine/tests/money.rs`

**Interfaces:**
- Produces: `Money` 派生 `Serialize`/`Deserialize`，序列化为裸 `i64`（分）。

- [ ] **Step 1: 写失败测试**

追加到 `packages/engine/tests/money.rs`（顶部加 `use serde_json::json;`）：
```rust
use serde_json::json;

#[test]
fn money_serde_roundtrip_preserves_cents() -> Result<(), Box<dyn std::error::Error>> {
    for cents in [0i64, 1, -1, 1234, 9_999_999, -5555] {
        let m = Money::from_cents(cents);
        let j = serde_json::to_value(m)?;
        assert_eq!(j, json!(cents), "serialize cents {cents}");
        let back: Money = serde_json::from_value(j)?;
        assert_eq!(back.cents(), cents, "deserialize cents {cents}");
    }
    Ok(())
}

#[test]
fn money_serde_is_bare_integer() -> Result<(), Box<dyn std::error::Error>> {
    // 前端拿到的应是裸整数，不是对象 {"cents": ...}
    let j = serde_json::to_value(Money::from_cents(42))?;
    assert_eq!(j, json!(42));
    assert!(j.as_i64() == Some(42)); // 不是 object
    Ok(())
}
```

- [ ] **Step 2: 运行测试，确认失败**

Run: `cargo test -p engine`
Expected: FAIL —— `Money` 未派生 `Serialize`/`Deserialize`，编译错误或 `to_value` 不存在（RED）。

- [ ] **Step 3: 写实现**

修改 `packages/engine/src/money.rs` 的 `Money` derive 行，加入 `serde::Serialize`/`Deserialize`：
```rust
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default,
    serde::Serialize, serde::Deserialize,
)]
pub struct Money(i64);
```
> 默认 newtype derive：`Money(1234)` 序列化为裸 `1234`，符合「直出 i64 分」。

- [ ] **Step 4: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/money.rs packages/engine/tests/money.rs
git commit -m "feat(engine): Money serde 直出 i64 分(往返保真)

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Task 7: 收尾 —— lib.rs 导出 + clippy + 全量回归

**Files:**
- Modify: `packages/engine/src/lib.rs`（确认 `pub use money::{Money, MoneyError};` 已就位）

- [ ] **Step 1: 写测试验证外部便捷导入可用**

追加到 `packages/engine/tests/money.rs`：
```rust
// 验证 lib.rs 的 re-export：调用方可直接 use engine::Money
#[test]
fn money_reexported_from_crate_root() {
    use engine::Money;
    assert_eq!(Money::ZERO.cents(), 0);
}
```

- [ ] **Step 2: 运行测试，确认通过**

Run: `cargo test -p engine`
Expected: PASS（若失败说明 lib.rs re-export 缺失，补 `pub use money::{Money, MoneyError};`）。

- [ ] **Step 3: clippy 与编译告警清零**

Run: `cargo clippy -p engine --all-targets -- -D warnings`
Expected: 无 warning / 无 clippy 提示（若有，按提示修正，如 `unwrap_or(i64::MAX)` 可能触发 clippy，可改为 `i64::from(shares)` 因 u32 安全转入 i64）。

**若 clippy 报 `mul_shares` 的 `try_from(...).unwrap_or(...)` 冗余**，改为：
```rust
    pub fn mul_shares(self, shares: u32) -> Result<Money, MoneyError> {
        self.0
            .checked_mul(i64::from(shares)) // u32 → i64 永不截断
            .map(Money)
            .ok_or_else(|| MoneyError::Overflow {
                op: "mul_shares",
                operand: format!("{} * {}", self.0, shares),
            })
    }
```
重跑 clippy 确认清零。

- [ ] **Step 4: 全量回归**

Run: `cargo test -p engine`
Expected: 全部 test passed（应 ≥ 20 个测试）。

Run: `cargo build -p engine`
Expected: 编译成功，无 warning。

- [ ] **Step 5: 提交**

```bash
git add packages/engine/src/lib.rs packages/engine/tests/money.rs packages/engine/src/money.rs
git commit -m "chore(engine): money 模块导出 + clippy 清零 + 全量回归绿

Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>"
```

---

## Self-Review（plan 作者自检）

**1. Spec 覆盖：**
- §2 决策1（i64 分存储）→ Task 2 ✓
- §2 决策2（价格资金同 Money）→ Task 2（注释说明）✓
- §2 决策3（比率 f64 + 银行家舍入）→ Task 5 ✓
- §2 决策4（可配置，不在本模块）→ spec §4 已声明边界，本 plan 不实现 GameConfig ✓（YAGNI）
- §3 API（from_cents/ZERO/cents/add/sub/mul_shares/from_yuan_str/apply_rate）→ Task 2/3/4/5 ✓
- §5 serde 直出 i64 → Task 6 ✓
- §6 错误处理三类 → Task 1 ✓
- §7 测试矩阵 7 大类 → 分散在 Task 1-6 测试中 ✓（构造、解析正确、解析防御、算术、银行家舍入、serde、派生语义）

**2. 占位扫描：** 无 TBD/TODO；每步含完整代码或命令 ✓。

**3. 类型一致性：**
- `MoneyError` 三变体在 Task 1 定义，Task 3/4/5 引用名称一致（`Overflow{op,operand}`、`ParseFailed{input,reason}`、`InvalidRate{rate}`）✓。
- `Money::from_cents` / `cents` / `ZERO` / `add` / `sub` / `mul_shares` / `from_yuan_str` / `apply_rate` 签名跨任务一致 ✓。
- `round_half_to_even` 模块级私有 fn，Task 5 定义并使用 ✓。

---

## Execution Handoff

Plan saved to `docs/superpowers/plans/2026-06-29-money-fixed-point.md`。

**用户已授权「直接运行到完成」** → 进入 ultracode 工作流，按 Task 1→7 顺序执行 TDD（红→绿→提交），每个任务结束跑 `cargo test -p engine` 验证。完成后如实汇报。
