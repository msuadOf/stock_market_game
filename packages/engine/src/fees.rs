//! 中立费用域：A 股交易费用三分项（佣金/印花税/过户费）。
//!
//! 搬迁背景（B2 批）：`FeeComponents` 原居 `session/pipeline/conservation.rs`
//! （旧管线），但费用分解是市场域（desk 扣费三分项）、个人域（交割单
//! `actual_fees`）与旧管线（守恒审计）的共用概念——按依赖方向放中立
//! 位置，`session/pipeline` 仅重导出不复制定义（不依赖逃生门）。
//!
//! 语义合同：
//! - 三分项单位均为分（`Money`）；
//! - `total()` 返回 `MoneyError`（中立域不认识 `StepFatal`——旧管线
//!   调用层自行映射，见 `conservation.rs` / `ledger_validation.rs`）；
//! - 与沪深现行费用结构的对应：commission=佣金（双向，有最低佣金）、
//!   stamp_tax=印花税（卖出单边）、transfer_fee=过户费（双向，沪市
//!   历史上单边——本游戏按现行过户费双向费率建模，登记于
//!   docs/trading-rules.md）。

use crate::money::{Money, MoneyError};

/// 一笔成交腿的实收费用三分项（分）。
#[derive(
    Clone, Copy, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS,
)]
#[ts(export)]
pub struct FeeComponents {
    /// 佣金（双向；按成交额比例、有最低佣金下限）。
    pub commission: Money,
    /// 印花税（卖出单边）。
    pub stamp_tax: Money,
    /// 过户费（双向）。
    pub transfer_fee: Money,
}

impl FeeComponents {
    /// 全零费用（无成交腿/免收费形态）。
    pub const ZERO: Self = Self {
        commission: Money::ZERO,
        stamp_tax: Money::ZERO,
        transfer_fee: Money::ZERO,
    };

    /// 三分项合计（溢出显式失败——中立域返回 `MoneyError`，调用层映射）。
    pub fn total(self) -> Result<Money, MoneyError> {
        self.commission
            .add(self.stamp_tax)
            .and_then(|fee| fee.add(self.transfer_fee))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_sums_three_components_without_rounding() {
        let fee = FeeComponents {
            commission: Money::from_cents(500),
            stamp_tax: Money::from_cents(100),
            transfer_fee: Money::from_cents(1),
        };
        assert_eq!(fee.total().unwrap(), Money::from_cents(601));
        assert_eq!(FeeComponents::ZERO.total().unwrap(), Money::ZERO);
    }

    #[test]
    fn total_overflow_is_an_explicit_money_error_not_a_panic() {
        // 佣金取 i64 上界、印花税再 +1：合计溢出必须显式 Err（防御式，
        // 不静默截断）。中立域不携带 StepFatal——错误类型为 MoneyError。
        let fee = FeeComponents {
            commission: Money::from_cents(i64::MAX),
            stamp_tax: Money::from_cents(1),
            transfer_fee: Money::ZERO,
        };
        assert!(fee.total().is_err());
    }
}
