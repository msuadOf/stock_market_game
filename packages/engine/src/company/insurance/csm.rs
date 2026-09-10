//! GMM 计量数学（K3 保险，任务 10）：整数半偶舍入除法孪生、简单贴现 PV、
//! 责任单元线性分摊（余数守恒）。
//!
//! 官方依据（已核验，docs/company-accounting.md §2.4）：CAS 25（2020）
//! §21 履约现金流量三元组（未来现金流量估计、货币时间价值及金融风险调整、
//! 非金融风险调整）、§23 无偏概率加权、§24 折现率、§25 风险调整、
//! §29–§32 责任单元释放。
//!
//! 游戏假设（版本化显式配置，Fixture 标注）：单一平坦年利率（bp）、
//! ACT/365F **简单贴现**（非复利期限结构）；风险调整不折现；预期赔付视为
//! 在保障期末一次性支付。全部假设不声称真实精算参数。

use crate::accounting::{AccountingError, FractionUnits};

/// 贴现分母基数（10_000bp × 365 天，ACT/365F）。
pub(super) const DISCOUNT_BASE: i128 = 3_650_000;

/// 整数半偶舍入除法（amount.rs / inventory / industrial::loans / bank::loans
/// 的同算法孪生；语义冻结区不改动）。
pub(super) fn rhe_div(n: i128, d: i128) -> Result<i128, AccountingError> {
    debug_assert!(d > 0, "divisor is positive by construction");
    let negative = n < 0;
    let numerator = n.unsigned_abs();
    let divisor = d.unsigned_abs();
    let quotient = numerator / divisor;
    let remainder = numerator % divisor;
    let doubled = remainder * 2;
    let round_up = doubled > divisor || (doubled == divisor && !quotient.is_multiple_of(2));
    let magnitude = i128::try_from(if round_up { quotient + 1 } else { quotient })
        .expect("quotient of |i128| by small constant fits i128");
    Ok(if negative { -magnitude } else { magnitude })
}

/// 简单贴现 PV = rhe(cents × 3_650_000 / (3_650_000 + rate_bp × days))。
/// rate_bp ∈ [1, 10000]、days ≥ 0 已由调用方守卫 ⇒ 分母 > 0。cents 可为负
/// （估计下调的 ΔPV），舍入对称。
pub(super) fn simple_discount_pv(
    cents: i128,
    rate_bp: i32,
    days: i64,
) -> Result<i128, AccountingError> {
    let divisor = DISCOUNT_BASE + i128::from(rate_bp) * i128::from(days);
    let scaled = cents
        .checked_mul(DISCOUNT_BASE)
        .ok_or(AccountingError::AmountOverflow {
            op: "discount pv",
            detail: format!("{cents} × {DISCOUNT_BASE}"),
        })?;
    rhe_div(scaled, divisor)
}

/// 责任单元分摊（守恒）：非末批 released = min(rhe((rem × units + carried) /
/// units_total), rem)；末批精确清零 released = rem、余数归零。不变量
/// released × units_total + 新余数 == rem × units + 旧余数（分毫不丢）。
/// 余数单位 = 分 × 单元（由调用点定义的 FractionUnits 语义）。
pub(super) fn unit_release(
    remaining_cents: i128,
    units: i64,
    units_total: i64,
    carried: FractionUnits,
    final_batch: bool,
) -> Result<(i128, FractionUnits), AccountingError> {
    debug_assert!(units > 0 && units_total > 0);
    if final_batch {
        return Ok((remaining_cents, FractionUnits::ZERO));
    }
    let scaled = remaining_cents
        .checked_mul(i128::from(units))
        .and_then(|v| v.checked_add(carried.units()))
        .ok_or(AccountingError::AmountOverflow {
            op: "unit release",
            detail: format!("{remaining_cents} × {units} + carried"),
        })?;
    let proposed = rhe_div(scaled, i128::from(units_total))?;
    let released = proposed.min(remaining_cents).max(0);
    let new_carried = scaled
        .checked_sub(
            released
                .checked_mul(i128::from(units_total))
                .expect("released × units_total fits i128"),
        )
        .ok_or(AccountingError::AmountOverflow {
            op: "unit release",
            detail: format!("remainder of {scaled}"),
        })?;
    Ok((released, FractionUnits::from_units(new_carried)))
}
