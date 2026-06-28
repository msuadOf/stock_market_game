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

/// 金额/股价的定点表示。内部恒为「分」(元×100) 的 i64，无 f64、无误差。
/// 有符号：盈亏/浮亏可为负。价格 = 每股元值，2 位小数，与资金同尺度。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, Default,
    serde::Serialize, serde::Deserialize,
)]
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
            .checked_mul(i64::from(shares))
            .map(Money)
            .ok_or_else(|| MoneyError::Overflow {
                op: "mul_shares",
                operand: format!("{} * {}", self.0, shares),
            })
    }

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
}

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
