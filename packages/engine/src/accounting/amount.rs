//! K2 公司会计金额：`AccountingAmount` = checked i128「分」。
//!
//! 与市场域 `Money`（i64 分）分离：公司域金额可超出 JS 安全整数，跨 JSON
//! 一律**十进制字符串**（绝不用 f64/JS number 承载）；互转必须显式范围检查。
//! 比例一律整数基点（1 bp = 1/10000）；利息小数经 [`FractionUnits`] 保留
//! 合同累计余数；付息/报表落分用同一个整数半偶舍入（round-half-to-even）。
//! 铁律：本模块无 f64，无静默 clamp，全部 checked。

use std::fmt;

use crate::accounting::error::AccountingError;
use crate::money::Money;

/// 基点分母（1 bp = 1/10000）。
const BP_DENOM: i128 = 10_000;

/// 会计金额：内部恒为「分」(元×100) 的 i128。有符号（亏损/负权益合法）。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
pub struct AccountingAmount(i128);

impl AccountingAmount {
    /// 零金额。
    pub const ZERO: Self = Self(0);
    /// 值域上界（i128::MAX 分）。
    pub const MAX: Self = Self(i128::MAX);

    /// 由「分」构造（零舍入）。
    pub fn from_cents(cents: i128) -> Self {
        Self(cents)
    }

    /// 内部「分」值。
    pub fn cents(self) -> i128 {
        self.0
    }

    pub fn is_zero(self) -> bool {
        self.0 == 0
    }

    pub fn is_positive(self) -> bool {
        self.0 > 0
    }

    pub fn is_negative(self) -> bool {
        self.0 < 0
    }

    /// checked 加法（溢出 → `AmountOverflow`）。与 `Money` 同约：不实现
    /// `std::ops::Add`，具名方法显式表达可失败。
    #[allow(clippy::should_implement_trait)]
    pub fn add(self, other: Self) -> Result<Self, AccountingError> {
        self.0
            .checked_add(other.0)
            .map(Self)
            .ok_or_else(|| overflow("add", format!("{} + {}", self.0, other.0)))
    }

    /// checked 减法（溢出 → `AmountOverflow`）。
    #[allow(clippy::should_implement_trait)]
    pub fn sub(self, other: Self) -> Result<Self, AccountingError> {
        self.0
            .checked_sub(other.0)
            .map(Self)
            .ok_or_else(|| overflow("sub", format!("{} - {}", self.0, other.0)))
    }

    /// checked 乘整数因子（数量/股数等，溢出 → `AmountOverflow`）。
    pub fn mul_i128(self, factor: i128) -> Result<Self, AccountingError> {
        self.0
            .checked_mul(factor)
            .map(Self)
            .ok_or_else(|| overflow("mul_i128", format!("{} * {}", self.0, factor)))
    }

    /// checked 取负（i128::MIN 无对应正数 → `AmountOverflow`）。
    #[allow(clippy::should_implement_trait)]
    pub fn neg(self) -> Result<Self, AccountingError> {
        self.0
            .checked_neg()
            .map(Self)
            .ok_or_else(|| overflow("neg", format!("-{}", self.0)))
    }

    /// 单次基点应用，整数半偶舍入落分（报表/一次性比例路径）。
    /// 需要跨期守恒的利息路径必须用 [`Self::apply_basis_points_accum`]。
    pub fn apply_basis_points(self, bp: i32) -> Result<Self, AccountingError> {
        let scaled = self.bp_product(bp)?;
        Ok(Self(div_round_half_even(scaled, BP_DENOM)))
    }

    /// 合同累计余数的基点应用：`paid = round_half_even((cents×bp + carried)/10000)`，
    /// `remainder` 回传继续累计。逐期不变量
    /// `Σpaid×10000 + 最终余数 == Σ(cents×bp)`：分毫不丢、不凭空造分。
    pub fn apply_basis_points_accum(
        self,
        bp: i32,
        carried: FractionUnits,
    ) -> Result<(Self, FractionUnits), AccountingError> {
        let scaled = self.bp_product(bp)?;
        let total = scaled
            .checked_add(carried.0)
            .ok_or_else(|| overflow("bp_accum", format!("{scaled} + {}", carried.0)))?;
        let paid = div_round_half_even(total, BP_DENOM);
        let remainder = total
            .checked_sub(
                paid.checked_mul(BP_DENOM)
                    .ok_or_else(|| overflow("bp_accum", format!("{paid} * {BP_DENOM}")))?,
            )
            .ok_or_else(|| overflow("bp_accum", format!("{total} - paid×{BP_DENOM}")))?;
        Ok((Self(paid), FractionUnits(remainder)))
    }

    /// 市场 `Money`（i64 分）→ 会计金额：i64 ⊂ i128，全域无损、不可失败。
    pub fn from_money(money: Money) -> Self {
        Self(i128::from(money.cents()))
    }

    /// 会计金额 → 市场 `Money`：超出 i64「分」值域显式拒绝（绝不截断）。
    pub fn to_money(self) -> Result<Money, AccountingError> {
        i64::try_from(self.0)
            .map(Money::from_cents)
            .map_err(|_| AccountingError::MoneyRangeExceeded { cents: self.0 })
    }

    fn bp_product(self, bp: i32) -> Result<i128, AccountingError> {
        self.0
            .checked_mul(i128::from(bp))
            .ok_or_else(|| overflow("apply_basis_points", format!("{} * {bp}", self.0)))
    }

    /// 人类可读「元」字符串（恰好 2 位小数，如 `-1234.56`）；serde 同此形态。
    pub fn to_yuan_string(self) -> String {
        let sign = if self.0 < 0 { "-" } else { "" };
        let abs = self.0.unsigned_abs();
        let yuan_part = abs / 100;
        let frac_part = abs % 100;
        format!("{sign}{yuan_part}.{frac_part:02}")
    }

    /// 严格解析「元」十进制字符串：`[+-]?数字[.1-2位]`；超 2 位小数/非数字/
    /// 空/多小数点/超 i128 → `DecimalParse`，绝不静默截断。
    pub fn from_yuan_str(input: &str) -> Result<Self, AccountingError> {
        let bad = |reason: &str| AccountingError::DecimalParse {
            input: input.to_string(),
            reason: reason.to_string(),
        };
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return Err(bad("empty input"));
        }
        let (negative, digits) = match trimmed.as_bytes()[0] {
            b'-' => (true, &trimmed[1..]),
            b'+' => (false, &trimmed[1..]),
            _ => (false, trimmed),
        };
        if digits.is_empty() {
            return Err(bad("sign without digits"));
        }
        let (int_part, frac_part) = match digits.split_once('.') {
            Some((i, f)) => {
                if f.contains('.') {
                    return Err(bad("multiple decimal points"));
                }
                if f.is_empty() {
                    return Err(bad("dangling decimal point"));
                }
                (i, f)
            }
            None => (digits, ""),
        };
        if frac_part.len() > 2 {
            return Err(bad(&format!(
                "too many fractional digits: {}",
                frac_part.len()
            )));
        }
        if !int_part.bytes().all(|b| b.is_ascii_digit()) || int_part.is_empty() {
            return Err(bad("integer part must be all digits"));
        }
        if !frac_part.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad("fractional part must be all digits"));
        }
        let int_cents: i128 = int_part
            .parse::<i128>()
            .map_err(|_| bad("integer part out of i128 range"))?;
        let frac_cents: i128 = match frac_part.len() {
            0 => 0,
            1 => i128::from(frac_part.as_bytes()[0] - b'0') * 10,
            _ => {
                let bytes = frac_part.as_bytes();
                i128::from((bytes[0] - b'0') * 10 + (bytes[1] - b'0'))
            }
        };
        let absolute = int_cents
            .checked_mul(100)
            .and_then(|c| c.checked_add(frac_cents))
            .ok_or_else(|| bad("value out of i128 range"))?;
        let signed = if negative {
            absolute.checked_neg()
        } else {
            Some(absolute)
        }
        .ok_or_else(|| bad("value out of i128 range"))?;
        Ok(Self(signed))
    }
}

/// 合同累计分数余数：单位 = 1/10000 分（基点应用的自然小数单位）。
/// 序列化为整数字符串（可能超 JS 安全整数，与金额同理由字符串承载）。
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default)]
pub struct FractionUnits(i128);

impl FractionUnits {
    pub const ZERO: Self = Self(0);

    pub fn from_units(units: i128) -> Self {
        Self(units)
    }

    pub fn units(self) -> i128 {
        self.0
    }
}

/// 整数半偶舍入除法 `n/d`（d > 0）：恰好 .5 时取最近偶数；负数对称。
/// 纯整数实现（u128 绝对值），无 f64。
fn div_round_half_even(n: i128, d: i128) -> i128 {
    debug_assert!(d > 0, "divisor is the constant BP_DENOM");
    let negative = n < 0;
    let numerator = n.unsigned_abs();
    let divisor = d.unsigned_abs();
    let quotient = numerator / divisor;
    let remainder = numerator % divisor;
    let doubled = remainder * 2;
    // 进位当且仅当：余数过半，或恰半而商为奇（半偶取偶）。
    let round_up = doubled > divisor || (doubled == divisor && !quotient.is_multiple_of(2));
    // 商 ≤ |i128::MIN|/10000 + 1，恒在 i128 值域内。
    let magnitude = i128::try_from(if round_up { quotient + 1 } else { quotient })
        .expect("quotient of i128 by 10000 always fits i128");
    if negative {
        -magnitude
    } else {
        magnitude
    }
}

fn overflow(op: &'static str, detail: String) -> AccountingError {
    AccountingError::AmountOverflow { op, detail }
}

/// Display = 「元」十进制字符串（与 serde 一致）。
impl fmt::Display for AccountingAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_yuan_string())
    }
}

/// Debug 人读：`AccountingAmount(-1234.56)`。
impl fmt::Debug for AccountingAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "AccountingAmount({})", self.to_yuan_string())
    }
}

/// serde = 十进制字符串（i128 超 JS 安全整数，禁用 JSON number）。
impl serde::Serialize for AccountingAmount {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_yuan_string())
    }
}

/// 反序列化经 `from_yuan_str` 全量校验——存档里的非法金额在恢复边界显式失败。
impl<'de> serde::Deserialize<'de> for AccountingAmount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::from_yuan_str(&text).map_err(serde::de::Error::custom)
    }
}

impl serde::Serialize for FractionUnits {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> serde::Deserialize<'de> for FractionUnits {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        text.parse::<i128>()
            .map(Self)
            .map_err(|_| serde::de::Error::custom("fraction units must be an integer string"))
    }
}
