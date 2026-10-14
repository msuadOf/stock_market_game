//! 交易部费用口径（照搬现引擎 `config.rs` + `pipeline/transition.rs`，逐分不差）。
//!
//! - 佣金双边：`max(成交额 × commission_rate, commission_min)`（有下限）。
//! - 印花税仅卖：`成交额 × stamp_tax_rate`（无下限）。
//! - 过户费双边：成交额 × 0.01‰（`A_SHARE_TRANSFER_FEE_RATE`）。
//! - 取整：`Money::apply_rate` 银行家舍入（round-half-to-even）——直接复用
//!   `GameConfig` 的计算函数保证同口径。
//! - 边际费用：部分成交逐笔累计时按 `fee_delta(before, after)` 增量计费。
//! - 卖腿实收封顶（ADR-0017 游戏简化）：实收费用 ≤ 本笔成交额，分摊顺序
//!   佣金 → 印花税 → 过户费，逐项 `min` 剩余额。

use crate::config::GameConfig;
use crate::money::{Money, MoneyError};
use crate::orderbook::Side;

/// 单边名义费用分项（不含封顶）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FeeQuote {
    /// 佣金。
    pub commission: Money,
    /// 印花税（买腿恒 0）。
    pub stamp_tax: Money,
    /// 过户费。
    pub transfer_fee: Money,
}

impl FeeQuote {
    /// 累计成交额的名义费用（0 额返回零费用，对齐 transition.rs nominal_total）。
    pub fn nominal(
        config: &GameConfig,
        side: Side,
        filled_value: Money,
    ) -> Result<Self, MoneyError> {
        if filled_value == Money::ZERO {
            return Ok(FeeQuote::default());
        }
        let fee = |f: fn(&GameConfig, Money) -> Result<Money, MoneyError>| f(config, filled_value);
        let _ = fee;
        let commission = config.commission(filled_value)?;
        let transfer_fee = config.transfer_fee(filled_value)?;
        let stamp_tax = match side {
            Side::Buy => Money::ZERO,
            Side::Sell => config.stamp_tax(filled_value)?,
        };
        Ok(FeeQuote {
            commission,
            stamp_tax,
            transfer_fee,
        })
    }

    /// 分项合计。
    pub fn total(self) -> Result<Money, MoneyError> {
        self.commission.add(self.stamp_tax)?.add(self.transfer_fee)
    }
}

/// 卖腿实收分配：本笔（增量）实收费用的分摊结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SellerCharge {
    /// 本笔实收佣金。
    pub commission: Money,
    /// 本笔实收印花税。
    pub stamp_tax: Money,
    /// 本笔实收过户费。
    pub transfer_fee: Money,
    /// 本笔净得（成交额 − 实收费用合计）。
    pub deliver_cash: Money,
}

/// 卖腿实收封顶分配（照搬 transition.rs `SellerChargeAllocation`）：
/// cap = min(未付费用合计, 本笔成交额)；顺序佣金 → 印花税 → 过户费逐项
/// `min` 剩余额；净得 = 成交额 − cap。
pub fn seller_charge(
    nominal_after: FeeQuote,
    charged_before: FeeQuote,
    gross_delta: Money,
) -> Result<SellerCharge, MoneyError> {
    let unpaid_commission = nominal_after.commission.sub(charged_before.commission)?;
    let unpaid_stamp = nominal_after.stamp_tax.sub(charged_before.stamp_tax)?;
    let unpaid_transfer = nominal_after
        .transfer_fee
        .sub(charged_before.transfer_fee)?;
    let unpaid_total = unpaid_commission.add(unpaid_stamp)?.add(unpaid_transfer)?;
    let cap = if unpaid_total.cents() < gross_delta.cents() {
        unpaid_total
    } else {
        gross_delta
    };
    let commission = Money::from_cents(unpaid_commission.cents().min(cap.cents()));
    let after_commission = Money::from_cents(cap.cents() - commission.cents());
    let stamp_tax = Money::from_cents(unpaid_stamp.cents().min(after_commission.cents()));
    let after_stamp = Money::from_cents(after_commission.cents() - stamp_tax.cents());
    let transfer_fee = Money::from_cents(unpaid_transfer.cents().min(after_stamp.cents()));
    let charged_total = commission.add(stamp_tax)?.add(transfer_fee)?;
    debug_assert_eq!(charged_total, cap, "分摊必须恰好耗尽 cap");
    let deliver_cash = gross_delta.sub(charged_total)?;
    Ok(SellerCharge {
        commission,
        stamp_tax,
        transfer_fee,
        deliver_cash,
    })
}

/// 买腿本笔应付：成交额 + 边际佣金 + 边际过户费（`fee_delta` 口径）。
pub fn buyer_charge(
    config: &GameConfig,
    filled_value_before: Money,
    gross_delta: Money,
) -> Result<Money, MoneyError> {
    let filled_after = filled_value_before.add(gross_delta)?;
    let commission = fee_delta(config_commission, config, filled_value_before, filled_after)?;
    let transfer = fee_delta(config_transfer, config, filled_value_before, filled_after)?;
    gross_delta.add(commission)?.add(transfer)
}

/// 买单全额预留口径（照搬 `config.rs buy_order_reservation`）：剩余对价 + 全程
/// 边际佣金 + 边际过户费。组端 purse 上限即此值。
pub fn buy_purse_limit(
    config: &GameConfig,
    limit: Money,
    qty: u32,
    filled_value: Money,
) -> Result<Money, MoneyError> {
    let remaining_gross = limit.mul_shares(qty)?;
    let final_gross = filled_value.add(remaining_gross)?;
    let commission = fee_delta(config_commission, config, filled_value, final_gross)?;
    let transfer = fee_delta(config_transfer, config, filled_value, final_gross)?;
    remaining_gross.add(commission)?.add(transfer)
}

/// 边际费用：`fee(after) − fee(before)`；before 为 0 时 before 费用记 0（对齐
/// `config.rs fee_delta`——0 额不触发下限）。
fn fee_delta(
    calc: fn(&GameConfig, Money) -> Result<Money, MoneyError>,
    config: &GameConfig,
    before: Money,
    after: Money,
) -> Result<Money, MoneyError> {
    let before_fee = if before == Money::ZERO {
        Money::ZERO
    } else {
        calc(config, before)?
    };
    calc(config, after)?.sub(before_fee)
}

fn config_commission(config: &GameConfig, amount: Money) -> Result<Money, MoneyError> {
    config.commission(amount)
}

fn config_transfer(config: &GameConfig, amount: Money) -> Result<Money, MoneyError> {
    config.transfer_fee(amount)
}
