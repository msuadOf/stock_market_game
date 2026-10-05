//! 费用与税费（工商会计约束）：人员/管理/销售/研发费用（现金）、增值税净额结算、
//! 当期 + 递延所得税计提与缴纳。
//!
//! 增值税：应纳 = 销项（222101 贷方）− 可抵扣进项（222102 借方）；进项富余
//! 结转留抵（不模拟退税）。所得税：期间税前利润（收入 − 费用，按总账期间
//! 索引）→ 亏损 FIFO 弥补（池在过账成功后落地）→ 当期税；递延所得税资产
//! 差额计入所得税费用（全额确认简化，CAS 18 受阻 docs §2.1）。
//! 标签映射：缴税 = TaxPayment/Operating；所得税计提 = TaxAccrual/NonCash。

use crate::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry, PostingSide,
};
use crate::calendar::CivilDate;
use crate::company::industrial::{chart, IndustrialBooks, IndustrialError};

/// 经营费用类别（人员费用暂走管理费用科目——简化：不单设应付职工薪酬 accrued
/// 循环；研发费用按财会〔2018〕15号单列口径）。
#[derive(Clone, Copy, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ExpenseKind {
    Staff,
    Admin,
    Selling,
    Rnd,
}

impl ExpenseKind {
    fn account_code(self) -> &'static str {
        match self {
            ExpenseKind::Staff | ExpenseKind::Admin => chart::acct::ADMIN_EXP,
            ExpenseKind::Selling => chart::acct::SELLING_EXP,
            ExpenseKind::Rnd => chart::acct::RND_EXP,
        }
    }
}

pub use crate::company::IncomeTaxOutcome;
impl IndustrialBooks {
    /// 现金费用：Dr 费用科目 / Cr 现金（经营活动）。资金不足 → `PaymentFailed`。
    pub fn pay_expense(
        &mut self,
        kind: ExpenseKind,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, IndustrialError> {
        if !amount.is_positive() {
            return Err(IndustrialError::NonPositiveAmount {
                what: "expense amount",
                amount,
            });
        }
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::CashExpense,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(kind.account_code(), PostingSide::Debit, amount),
                    super::line(chart::acct::BANK, PostingSide::Credit, amount),
                ],
            }],
        )?;
        Ok(event)
    }

    /// 缴纳增值税净额：Dr 销项 / Cr 进项（净额对冲）/ Cr 现金（应纳部分，
    /// 经营活动）。无销项税额 → `NothingToPay`（显式拒绝而非静默 no-op；
    /// 进项富余自然留抵 222102 借方，不模拟退税）。
    pub fn pay_vat(&mut self, date: CivilDate) -> Result<BusinessEventId, IndustrialError> {
        let output = self.net_of(chart::acct::VAT_OUT)?.neg()?;
        let input = self.net_of(chart::acct::VAT_IN)?;
        if output.is_zero() {
            return Err(IndustrialError::NothingToPay);
        }
        let offset = if output < input { output } else { input };
        let payable = output.sub(offset)?;
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        let mut lines = vec![super::line(
            chart::acct::VAT_OUT,
            PostingSide::Debit,
            output,
        )];
        if offset.is_positive() {
            lines.push(super::line(
                chart::acct::VAT_IN,
                PostingSide::Credit,
                offset,
            ));
        }
        if payable.is_positive() {
            lines.push(super::line(chart::acct::BANK, PostingSide::Credit, payable));
        }
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::TaxPayment,
                cash_flow: CashFlowClass::Operating,
                lines,
            }],
        )?;
        Ok(event)
    }

    /// 年度累计计提及后续年度级联；差额在当前开放期间过账。
    pub fn accrue_income_tax(
        &mut self,
        date: CivilDate,
    ) -> Result<IncomeTaxOutcome, IndustrialError> {
        self.reassess_income_tax(date.year(), date, &std::collections::BTreeMap::new())
    }

    /// 缴纳所得税：Dr 应交所得税 / Cr 现金（经营活动）；超应纳 → 类型化拒绝。
    pub fn pay_income_tax(
        &mut self,
        amount: AccountingAmount,
        date: CivilDate,
    ) -> Result<BusinessEventId, IndustrialError> {
        if !amount.is_positive() {
            return Err(IndustrialError::NonPositiveAmount {
                what: "income tax payment",
                amount,
            });
        }
        let payable = self.net_of(chart::acct::CIT_PAYABLE)?.neg()?;
        if amount > payable {
            return Err(IndustrialError::TaxOverpayment {
                requested: amount,
                payable,
            });
        }
        let base = self.next_event_id;
        let event = BusinessEventId::new(base);
        self.post_with_commit(
            base + 1,
            vec![JournalEntry {
                source: event,
                date,
                kind: BusinessKind::TaxPayment,
                cash_flow: CashFlowClass::Operating,
                lines: vec![
                    super::line(chart::acct::CIT_PAYABLE, PostingSide::Debit, amount),
                    super::line(chart::acct::BANK, PostingSide::Credit, amount),
                ],
            }],
        )?;
        Ok(event)
    }
}
