//! 项目借款状态与 ACT/365F 计息数学（K3 地产，`borrowing_costs.rs` 的
//! 状态/数学半边；结构与 `industrial::loans` / `bank::loans` 孪生同构）。
//!
//! 计息：interest = rhe((本金×利率bp×天数 + 余数) / 3_650_000)，余数以任务 6
//! [`FractionUnits`] 承载（单位 = 1/3_650_000 分）。地产特有：**资本化与
//! 费用化是两条独立余数链**——逐链不变量 Σpaid×3_650_000 + 终余数 ==
//! Σ(本金×bp×链内天数)，两条链合计恒等于合同全期利息，分毫不丢。

use std::collections::BTreeMap;

use crate::accounting::{AccountingAmount, AccountingError, FractionUnits};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::real_estate::chart;
use crate::company::real_estate::projects::ProjectId;

/// ACT/365F 分母（10_000 bp × 365 天）。
const ACT_365F_DIVISOR: i128 = 3_650_000;

/// 单笔项目借款状态：未偿本金、已提未付利息（资本化 + 费用化共用 2231）、
/// 两条计息余数链、上次计提日、贷款人、指定项目（None = 无项目借款，
/// 恒费用化）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ProjectLoanState {
    outstanding: AccountingAmount,
    accrued_unpaid: AccountingAmount,
    /// 资本化链余数（单位 1/3_650_000 分）。
    carried_cap: FractionUnits,
    /// 费用化链余数。
    carried_exp: FractionUnits,
    last_accrual_date: CivilDate,
    lender: crate::company::counterparty::CounterpartyId,
    project: Option<ProjectId>,
    /// 过账借款科目（2001/2501，按起息—到期期限在借款时选定）。
    debt_account: String,
    annual_rate_bp: i32,
}

impl ProjectLoanState {
    pub(super) fn new(
        principal: AccountingAmount,
        annual_rate_bp: i32,
        start: CivilDate,
        lender: crate::company::counterparty::CounterpartyId,
        project: Option<ProjectId>,
        debt_account: &'static str,
    ) -> Self {
        Self {
            outstanding: principal,
            accrued_unpaid: AccountingAmount::ZERO,
            carried_cap: FractionUnits::ZERO,
            carried_exp: FractionUnits::ZERO,
            last_accrual_date: start,
            lender,
            project,
            debt_account: debt_account.to_string(),
            annual_rate_bp,
        }
    }

    pub fn outstanding(&self) -> AccountingAmount {
        self.outstanding
    }

    pub fn accrued_unpaid(&self) -> AccountingAmount {
        self.accrued_unpaid
    }

    pub fn carried_cap(&self) -> FractionUnits {
        self.carried_cap
    }

    pub fn carried_exp(&self) -> FractionUnits {
        self.carried_exp
    }

    pub fn last_accrual_date(&self) -> CivilDate {
        self.last_accrual_date
    }

    pub fn lender(&self) -> &crate::company::counterparty::CounterpartyId {
        &self.lender
    }

    pub fn project(&self) -> Option<&ProjectId> {
        self.project.as_ref()
    }

    pub fn annual_rate_bp(&self) -> i32 {
        self.annual_rate_bp
    }

    pub(super) fn debt_account(&self) -> &str {
        &self.debt_account
    }

    /// 落地一条计提（金额为零也推进余数与计提日——守恒所需）。
    pub(super) fn apply_split(
        &mut self,
        item: &InterestSplitItem,
        through: CivilDate,
    ) -> Result<(), AccountingError> {
        self.accrued_unpaid = self
            .accrued_unpaid
            .add(item.capitalized_amount)?
            .add(item.expensed_amount)?;
        self.carried_cap = item.carried_cap;
        self.carried_exp = item.carried_exp;
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

/// 单合同计提结果（天数按资本化窗口拆分为两条链）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct InterestSplitItem {
    pub contract: ContractId,
    pub days: i64,
    pub capitalized_days: i64,
    pub expensed_days: i64,
    pub capitalized_amount: AccountingAmount,
    pub expensed_amount: AccountingAmount,
    pub carried_cap: FractionUnits,
    pub carried_exp: FractionUnits,
}

/// 计提落地的通用入口（无该合同则 no-op——仅由过账成功路径调用）。
pub(super) fn apply_split(
    loans: &mut BTreeMap<ContractId, ProjectLoanState>,
    item: &InterestSplitItem,
    through: CivilDate,
) -> Result<(), AccountingError> {
    if let Some(state) = loans.get_mut(&item.contract) {
        state.apply_split(item, through)?;
    }
    Ok(())
}

/// 借款科目：到期 − 起息 ≤ 365 天 → 短期借款；否则长期借款。
pub(super) fn loan_account(start: CivilDate, maturity: CivilDate) -> &'static str {
    if maturity.days_since(start) <= 365 {
        chart::acct::ST_DEBT
    } else {
        chart::acct::LT_DEBT
    }
}

/// ACT/365F 单链计提（任务 6 `apply_basis_points_accum` 同构）：
/// paid = rhe((cents×bp×days + carried) / 3_650_000)；remainder 继续累计。
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

/// 整数半偶舍入除法（projects.rs 孪生同算法；见彼处注释）。
pub(super) fn rhe_div(n: i128, d: i128) -> Result<i128, AccountingError> {
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
