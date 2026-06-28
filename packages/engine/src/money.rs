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
}
