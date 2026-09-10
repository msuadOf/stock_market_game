//! 借款状态与 ACT/365F 计息数学（K3 工商，`interest.rs` 的状态/数学半边）。
//!
//! 计息：interest = rhe((本金×利率bp×天数 + 余数) / 3_650_000)，余数以任务 6
//! [`FractionUnits`] 承载（此处单位 = 1/3_650_000 分，ACT/365F 的自然小数单位；
//! docs/simulation-calendar.md §6）。逐合同不变量 Σ已提 × 3_650_000 + 终余数 ==
//! Σ(本金×bp×天数)——分毫不丢、不凭空造分（与任务 6 `apply_basis_points_accum`
//! 同构，任务 9–11 银行/保险计息复用同一约定）。

use std::collections::BTreeMap;

use crate::accounting::{AccountingAmount, AccountingError, BusinessEventId, FractionUnits};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::industrial::chart;

/// 开局借款隐式合同 id（与 2001 开局余额一一对应；task-7 review O2 治理决策）。
pub const OPENING_DEBT_CONTRACT_ID: &str = "OPENING-DEBT";

/// ACT/365F 分母（10_000 bp × 365 天）。
const ACT_365F_DIVISOR: i128 = 3_650_000;

/// 单合同借款状态：未偿本金、已提未付利息、计息余数、上次计提日。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct LoanState {
    outstanding: AccountingAmount,
    accrued_unpaid: AccountingAmount,
    /// ACT/365F 计息余数（单位 1/3_650_000 分；随合同累计，落分守恒）。
    carried: FractionUnits,
    last_accrual_date: CivilDate,
}

impl LoanState {
    pub(super) fn new(principal: AccountingAmount, start: CivilDate) -> Self {
        Self {
            outstanding: principal,
            accrued_unpaid: AccountingAmount::ZERO,
            carried: FractionUnits::ZERO,
            last_accrual_date: start,
        }
    }

    pub fn outstanding(&self) -> AccountingAmount {
        self.outstanding
    }

    pub fn accrued_unpaid(&self) -> AccountingAmount {
        self.accrued_unpaid
    }

    pub fn carried(&self) -> FractionUnits {
        self.carried
    }

    pub fn last_accrual_date(&self) -> CivilDate {
        self.last_accrual_date
    }

    /// 落地一条计提（金额为零也推进余数与计提日——守恒所需）。
    pub(super) fn apply_accrual_item(
        &mut self,
        item: &InterestAccrualItem,
        through: CivilDate,
    ) -> Result<(), AccountingError> {
        self.accrued_unpaid = self.accrued_unpaid.add(item.amount)?;
        self.carried = item.remaining_carried;
        self.last_accrual_date = through;
        Ok(())
    }

    /// 付清已提未付利息。
    pub(super) fn settle_accrued(&mut self) {
        self.accrued_unpaid = AccountingAmount::ZERO;
    }

    /// 归还本金（调用方已验证金额 ≤ 未偿）。
    pub(super) fn repay(&mut self, amount: AccountingAmount) -> Result<(), AccountingError> {
        self.outstanding = self.outstanding.sub(amount)?;
        Ok(())
    }
}

/// 借款表落地的通用入口（无该合同则 no-op——仅由过账成功路径调用）。
pub(super) fn apply_accrual(
    loans: &mut BTreeMap<ContractId, LoanState>,
    item: &InterestAccrualItem,
    through: CivilDate,
) -> Result<(), AccountingError> {
    if let Some(state) = loans.get_mut(&item.contract) {
        state.apply_accrual_item(item, through)?;
    }
    Ok(())
}

/// 单合同计提结果。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct InterestAccrualItem {
    pub contract: ContractId,
    pub days: i64,
    pub amount: AccountingAmount,
    pub remaining_carried: FractionUnits,
}

/// 还本结果（先计提后还本 ⇒ 1–2 张分录）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct RepaymentOutcome {
    pub events: Vec<BusinessEventId>,
}

/// 借款科目：到期 − 起息 ≤ 365 天 → 短期借款；否则长期借款。
pub(super) fn loan_account(start: CivilDate, maturity: CivilDate) -> &'static str {
    if maturity.days_since(start) <= 365 {
        chart::acct::ST_DEBT
    } else {
        chart::acct::LT_DEBT
    }
}

/// ACT/365F 单期计提（任务 6 `apply_basis_points_accum` 同构）：
/// paid = rhe((cents×bp×days + carried) / 3_650_000)；remainder 继续累计。
/// 不变量 Σpaid×3_650_000 + 终余数 == Σ(cents×bp×days)。
pub(super) fn accrue_act_365f(
    principal: AccountingAmount,
    rate_bp: i32,
    days: i64,
    carried: FractionUnits,
) -> Result<(AccountingAmount, FractionUnits), AccountingError> {
    let scaled = principal
        .cents()
        .checked_mul(i128::from(rate_bp))
        .and_then(|v| v.checked_mul(i128::from(days)))
        .and_then(|v| v.checked_add(carried.units()))
        .ok_or(AccountingError::AmountOverflow {
            op: "act_365f accrual",
            detail: format!("{} × {rate_bp}bp × {days}d", principal.cents()),
        })?;
    let paid = rhe_div(scaled, ACT_365F_DIVISOR)?;
    let remainder = scaled
        .checked_sub(
            paid.checked_mul(ACT_365F_DIVISOR)
                .expect("product of rounded quotient by divisor fits i128"),
        )
        .ok_or(AccountingError::AmountOverflow {
            op: "act_365f accrual",
            detail: format!("remainder of {scaled}"),
        })?;
    Ok((
        AccountingAmount::from_cents(paid),
        FractionUnits::from_units(remainder),
    ))
}

/// 整数半偶舍入除法（任务 6 `amount.rs` 私有实现与 accounting::inventory 共享
/// 副本的同算法孪生——journal/ledger/amount 属任务 6 语义冻结区，不改动）。
fn rhe_div(n: i128, d: i128) -> Result<i128, AccountingError> {
    debug_assert!(d > 0, "divisor is the ACT/365F constant");
    let negative = n < 0;
    let numerator = n.unsigned_abs();
    let divisor = d.unsigned_abs();
    let quotient = numerator / divisor;
    let remainder = numerator % divisor;
    let doubled = remainder * 2;
    let round_up = doubled > divisor || (doubled == divisor && !quotient.is_multiple_of(2));
    let magnitude = i128::try_from(if round_up { quotient + 1 } else { quotient })
        .expect("quotient of |i128| by 3_650_000 fits i128");
    Ok(if negative { -magnitude } else { magnitude })
}
