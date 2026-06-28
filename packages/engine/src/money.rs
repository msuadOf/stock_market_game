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

/// 占位：Task 2 实现真实 Money。本行仅让 lib.rs 的 pub use 编译通过。
#[derive(Debug)]
pub struct Money(i64);
