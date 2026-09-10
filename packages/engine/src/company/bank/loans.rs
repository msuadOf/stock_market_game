//! 贷款子账状态与计息数学（K3 银行，任务 9）：`BankLoanState`（本金/应收
//! 利息/计息余数/ECL 阶段与准备/核销回收面）+ ACT/365F 数学 + 合同条款
//! 校验。发放/收本处理器在 `lending.rs`、计息在 `interest.rs`、ECL 计量
//! 在 `ecl.rs`、核销回收在 `writeoff.rs`。
//!
//! 计息数学：ACT/365F + 合同累计余数（`FractionUnits`，单位 1/3_650_000 分）
//! ——与任务 8 `industrial::loans::accrue_act_365f` 同算法孪生（该函数为
//! `pub(super)` 私有，本任务禁改 industrial，故按既有 rhe_div 孪生先例复制，
//! 单位语义由调用点定义）。逐合同不变量
//! Σ已提 × 3_650_000 + 终余数 == Σ(基数×bp×天数)。
//!
//! 简化登记：逾期贷款按合同利率继续计息（无罚息利率模型）；单利不计复利
//! （与任务 8 一致）。

use crate::accounting::{AccountingAmount, AccountingError, FractionUnits};
use crate::calendar::CivilDate;
use crate::company::bank::ecl::{EclStage, StageTransferRecord};
use crate::company::bank::BankError;
use crate::company::contracts::ContractId;
use crate::company::counterparty::CounterpartyId;

/// ACT/365F 分母（10_000bp × 365 天）。
const ACT_365F_DIVISOR: i128 = 3_650_000;

/// 单笔贷款状态：本金、应收利息、计息余数、ECL 阶段与准备、核销/回收面。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct BankLoanState {
    principal: AccountingAmount,
    accrued_receivable: AccountingAmount,
    /// ACT/365F 计息余数（单位 1/3_650_000 分；随合同累计，落分守恒）。
    carried: FractionUnits,
    last_accrual_date: CivilDate,
    pub(super) rate_bp: i32,
    pub(super) counterparty: CounterpartyId,
    stage: EclStage,
    /// 该贷款的减值准备（子账事实；总账 1303 = Σ各贷款准备）。
    pub(super) allowance: AccountingAmount,
    written_off: bool,
    /// 已核销尚未回收的账面余额（回收上限）。
    recoverable: AccountingAmount,
    transfers: Vec<StageTransferRecord>,
}

impl BankLoanState {
    pub(super) fn new(
        principal: AccountingAmount,
        rate_bp: i32,
        counterparty: CounterpartyId,
        start: CivilDate,
    ) -> Self {
        Self {
            principal,
            accrued_receivable: AccountingAmount::ZERO,
            carried: FractionUnits::ZERO,
            last_accrual_date: start,
            rate_bp,
            counterparty,
            stage: EclStage::Stage1,
            allowance: AccountingAmount::ZERO,
            written_off: false,
            recoverable: AccountingAmount::ZERO,
            transfers: Vec::new(),
        }
    }

    pub fn principal(&self) -> AccountingAmount {
        self.principal
    }

    pub fn accrued_receivable(&self) -> AccountingAmount {
        self.accrued_receivable
    }

    pub fn carried(&self) -> FractionUnits {
        self.carried
    }

    pub fn last_accrual_date(&self) -> CivilDate {
        self.last_accrual_date
    }

    pub fn stage(&self) -> EclStage {
        self.stage
    }

    pub fn allowance(&self) -> AccountingAmount {
        self.allowance
    }

    pub fn is_written_off(&self) -> bool {
        self.written_off
    }

    pub fn recoverable(&self) -> AccountingAmount {
        self.recoverable
    }

    pub fn stage_transfers(&self) -> &[StageTransferRecord] {
        &self.transfers
    }

    /// 账面余额（毛额）= 本金 + 应计利息（EAD 与计息基数口径，CAS 22 §61）。
    pub fn gross_carrying(&self) -> AccountingAmount {
        self.principal
            .add(self.accrued_receivable)
            .expect("gross carrying amount within checked i128")
    }

    /// 第三阶段净额法计息基数 = max(账面余额 − 准备, 0)。准备可能临时超过
    /// 账面余额（回收后未重估）——显式零下限规则（回收后重估转回，见 ecl.rs）。
    pub(super) fn net_accrual_base(&self) -> AccountingAmount {
        match self.gross_carrying().sub(self.allowance) {
            Ok(net) if net.is_positive() => net,
            _ => AccountingAmount::ZERO,
        }
    }

    pub(super) fn apply_accrual(
        &mut self,
        amount: AccountingAmount,
        remaining: FractionUnits,
        through: CivilDate,
    ) -> Result<(), AccountingError> {
        self.accrued_receivable = self.accrued_receivable.add(amount)?;
        self.carried = remaining;
        self.last_accrual_date = through;
        Ok(())
    }

    pub(super) fn apply_assessment(
        &mut self,
        target: AccountingAmount,
        date: CivilDate,
        stage: EclStage,
        reason: &str,
    ) {
        let from = self.stage;
        if from != stage {
            self.transfers.push(StageTransferRecord {
                date,
                from_stage: from,
                to_stage: stage,
                reason: reason.to_string(),
            });
            self.stage = stage;
        }
        self.allowance = target;
    }

    pub(super) fn collect_principal(
        &mut self,
        amount: AccountingAmount,
    ) -> Result<(), AccountingError> {
        self.principal = self.principal.sub(amount)?;
        Ok(())
    }

    pub(super) fn collect_interest(
        &mut self,
        amount: AccountingAmount,
    ) -> Result<(), AccountingError> {
        self.accrued_receivable = self.accrued_receivable.sub(amount)?;
        Ok(())
    }

    pub(super) fn apply_write_off(&mut self) -> Result<(), AccountingError> {
        let gross = self.gross_carrying();
        self.principal = AccountingAmount::ZERO;
        self.accrued_receivable = AccountingAmount::ZERO;
        self.allowance = AccountingAmount::ZERO;
        self.recoverable = gross;
        self.written_off = true;
        Ok(())
    }

    pub(super) fn apply_recovery(
        &mut self,
        amount: AccountingAmount,
    ) -> Result<(), AccountingError> {
        self.recoverable = self.recoverable.sub(amount)?;
        self.allowance = self.allowance.add(amount)?;
        Ok(())
    }
}

/// 整数半偶舍入除法（amount.rs / inventory / industrial::loans 的同算法孪生；
/// journal/ledger/amount 属任务 6 语义冻结区，不改动）。
pub(super) fn rhe_div(n: i128, d: i128) -> Result<i128, AccountingError> {
    debug_assert!(d > 0, "divisor is a positive constant");
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

/// ACT/365F 单期计提（任务 8 同构）：paid = rhe((cents×bp×days + carried) /
/// 3_650_000)；remainder 继续累计。
pub(super) fn accrue_act_365f(
    base: AccountingAmount,
    rate_bp: i32,
    days: i64,
    carried: FractionUnits,
) -> Result<(AccountingAmount, FractionUnits), AccountingError> {
    let scaled = base
        .cents()
        .checked_mul(i128::from(rate_bp))
        .and_then(|v| v.checked_mul(i128::from(days)))
        .and_then(|v| v.checked_add(carried.units()))
        .ok_or(AccountingError::AmountOverflow {
            op: "act_365f accrual",
            detail: format!("{} × {rate_bp}bp × {days}d", base.cents()),
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

/// 合同条款校验（存款/贷款共用形状）：正本金、非负利率、到期晚于起息日。
pub(super) fn validate_terms(
    contract: &ContractId,
    principal: AccountingAmount,
    rate_bp: i32,
    start: CivilDate,
    maturity: CivilDate,
) -> Result<(), BankError> {
    if !principal.is_positive() {
        return Err(BankError::NonPositiveAmount {
            what: "principal",
            amount: principal,
        });
    }
    if rate_bp < 0 {
        return Err(BankError::InvalidRateBp { rate_bp });
    }
    if maturity <= start {
        return Err(BankError::MaturityNotAfterStart {
            contract: contract.clone(),
            start,
            maturity,
        });
    }
    Ok(())
}
