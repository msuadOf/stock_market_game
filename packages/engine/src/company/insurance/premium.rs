//! 保费子账与处理器（K3 保险，任务 10；CAS 25 §27 初始确认）。
//!
//! 初始计量（GMM，整数）：PV = rhe(预期赔付 × 3_650_000 / (3_650_000 +
//! rate_bp × 保障天数))；CSM₀ = 保费 − PV − 风险调整（负 → 首日亏损即期
//! 入损益，CAS 25 §27/§46）；F（保险财务损益总额）= 预期赔付 − PV（随责任
//! 单元回拨）。保费挂应收（1122）并贷记未到期责任负债（2501）——**保费不是
//! 收入**（K3 红线），收入随责任单元释放（service_release.rs）。
//!
//! 简化登记：保费于建立日一次性挂账（PV 不折现）；风险调整不折现。

use crate::accounting::{
    AccountingAmount, AccountingError, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry,
    PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::counterparty::FlowDirection;
use crate::company::insurance::csm;
use crate::company::insurance::groups::ContractGroupState;
use crate::company::insurance::{chart, InsuranceBooks, InsuranceError, InsuranceProductKind};

/// 初始计量结果：CSM₀（可为负 = 首日亏损）与贴现差 F。
pub(super) struct InitialMeasurement {
    pub csm_initial: i128,
    pub finance_total: i128,
}

/// 初始计量：CSM₀ = premium − PV(claims) − RA；F = claims − PV(claims)。
pub(super) fn initial_measurement(
    premium: AccountingAmount,
    expected_claims: AccountingAmount,
    risk_adjustment: AccountingAmount,
    rate_bp: i32,
    coverage_days: i64,
) -> Result<InitialMeasurement, InsuranceError> {
    let pv = csm::simple_discount_pv(expected_claims.cents(), rate_bp, coverage_days)?;
    let finance_total = expected_claims.cents() - pv;
    let csm_initial = premium
        .cents()
        .checked_sub(pv)
        .and_then(|v| v.checked_sub(risk_adjustment.cents()))
        .ok_or(InsuranceError::Accounting(
            AccountingError::AmountOverflow {
                op: "initial csm",
                detail: format!("{} − {pv} − {}", premium.cents(), risk_adjustment.cents()),
            },
        ))?;
    Ok(InitialMeasurement {
        csm_initial,
        finance_total,
    })
}

impl InsuranceBooks {
    /// 建立合同组（CAS 25 §11/§12/§20 分组 + §27 初始确认）：
    /// 1. 产品种类守卫（仅 TermProtection——分红/投连/再保险类型化拒绝）；
    /// 2. 金额/日期/重复守卫；
    /// 3. GMM 初始计量（CSM₀ / 首日亏损 / F）；
    /// 4. 原子过账：应收保费挂账（+ 亏损组首日亏损），全部非现金；
    /// 5. 子账落地。
    #[allow(clippy::too_many_arguments)]
    pub fn establish_group(
        &mut self,
        kind: InsuranceProductKind,
        group: ContractId,
        policyholder: &crate::company::counterparty::CounterpartyId,
        premium: AccountingAmount,
        expected_claims: AccountingAmount,
        risk_adjustment: AccountingAmount,
        start: CivilDate,
        end: CivilDate,
    ) -> Result<BusinessEventId, InsuranceError> {
        kind.require_supported()?;
        if !premium.is_positive() {
            return Err(InsuranceError::NonPositiveAmount {
                what: "premium",
                amount: premium,
            });
        }
        if !expected_claims.is_positive() {
            return Err(InsuranceError::NonPositiveAmount {
                what: "expected claims",
                amount: expected_claims,
            });
        }
        if risk_adjustment.is_negative() {
            return Err(InsuranceError::NonPositiveAmount {
                what: "risk adjustment",
                amount: risk_adjustment,
            });
        }
        if end <= start {
            return Err(InsuranceError::CoverageEndNotAfterStart {
                group: group.clone(),
                start,
                end,
            });
        }
        self.ensure_counterparty(policyholder)?;
        if self.groups.contains_key(&group) {
            return Err(InsuranceError::DuplicateGroup { group });
        }
        let units_total = end.days_since(start);
        let measured = initial_measurement(
            premium,
            expected_claims,
            risk_adjustment,
            self.discount.rate_bp,
            units_total,
        )?;
        let (csm, loss) = if measured.csm_initial >= 0 {
            (
                AccountingAmount::from_cents(measured.csm_initial),
                AccountingAmount::ZERO,
            )
        } else {
            (
                AccountingAmount::ZERO,
                AccountingAmount::from_cents(measured.csm_initial.checked_neg().ok_or(
                    InsuranceError::Accounting(AccountingError::AmountOverflow {
                        op: "day-one loss",
                        detail: format!("neg of {}", measured.csm_initial),
                    }),
                )?),
            )
        };
        let event = BusinessEventId::new(self.next_event_id);
        let mut entries = vec![JournalEntry {
            source: event,
            date: start,
            kind: BusinessKind::InsurancePremiumAccrued,
            cash_flow: CashFlowClass::NonCash,
            lines: vec![
                super::line(chart::acct::PREMIUM_RECEIVABLE, PostingSide::Debit, premium),
                super::line(chart::acct::LRC, PostingSide::Credit, premium),
            ],
        }];
        let allocated = if loss.is_zero() {
            1
        } else {
            entries.push(JournalEntry {
                source: BusinessEventId::new(self.next_event_id + 1),
                date: start,
                kind: BusinessKind::InsuranceLossComponent,
                cash_flow: CashFlowClass::NonCash,
                lines: vec![
                    super::line(chart::acct::INSURANCE_EXPENSE, PostingSide::Debit, loss),
                    super::line(chart::acct::LRC, PostingSide::Credit, loss),
                ],
            });
            2
        };
        self.post_with_commit(self.next_event_id + allocated, entries)?;
        self.groups.insert(
            group.clone(),
            ContractGroupState::new(
                policyholder.clone(),
                premium,
                expected_claims,
                risk_adjustment,
                csm,
                loss,
                AccountingAmount::from_cents(measured.finance_total),
                start,
                end,
                units_total,
            ),
        );
        Ok(event)
    }

    /// 保费收讫（现金入、应收清零，不重复计负债）：Dr 1002 / Cr 1122（经营）。
    /// 超未收余额 → `PremiumBeyondReceivable`。
    pub fn collect_premium(
        &mut self,
        group: &ContractId,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, InsuranceError> {
        let state = self.group(group).ok_or(InsuranceError::UnknownGroup {
            group: group.clone(),
        })?;
        if !amount.is_positive() {
            return Err(InsuranceError::NonPositiveAmount {
                what: "premium collection",
                amount,
            });
        }
        let outstanding = state.premium_outstanding()?;
        if amount > outstanding {
            return Err(InsuranceError::PremiumBeyondReceivable {
                group: group.clone(),
                requested: amount,
                outstanding,
            });
        }
        let policyholder = state.policyholder().clone();
        let event = BusinessEventId::new(self.next_event_id);
        self.post_with_commit(
            self.next_event_id + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::InsurancePremiumCollected,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::CASH, PostingSide::Debit, amount),
                    super::line(chart::acct::PREMIUM_RECEIVABLE, PostingSide::Credit, amount),
                ],
            }],
        )?;
        if let Some(state) = self.groups.get_mut(group) {
            state.apply_premium_collection(amount)?;
        }
        self.record_flow(
            date,
            &policyholder,
            FlowDirection::Inbound,
            amount,
            "premium collected",
        )?;
        Ok(event)
    }
}
