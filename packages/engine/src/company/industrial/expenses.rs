//! 费用与税费（K3 工商）：人员/管理/销售/研发费用（现金）、增值税净额结算、
//! 当期 + 递延所得税计提与缴纳。
//!
//! 增值税：应纳 = 销项（222101 贷方）− 可抵扣进项（222102 借方）；进项富余
//! 结转留抵（不模拟退税）。所得税：期间税前利润（收入 − 费用，按总账期间
//! 索引）→ 亏损 FIFO 弥补（池在过账成功后落地）→ 当期税；递延所得税资产
//! 差额计入所得税费用（全额确认简化，CAS 18 受阻 docs §2.1）。
//! 标签映射：缴税 = TaxPayment/Operating；所得税计提 = TaxAccrual/NonCash。

use crate::accounting::{
    compute_income_tax, AccountElement, AccountingAmount, AccountingPeriod, BusinessEventId,
    BusinessKind, CashFlowClass, JournalEntry, PostingSide,
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

/// 所得税计提结果。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct IncomeTaxOutcome {
    /// 计提分录事件（零税前且无递延变动时无分录 = None）。
    pub event: Option<BusinessEventId>,
    pub pretax: AccountingAmount,
    pub current_tax: AccountingAmount,
    pub loss_offset_used: AccountingAmount,
    pub loss_added: AccountingAmount,
    pub losses_expired: AccountingAmount,
    /// 递延所得税资产变动（正 = 确认，负 = 转回）。
    pub deferred_delta: AccountingAmount,
}

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

    /// 计提所得税（当期 + 递延）：**年度**税前利润取自总账全年期间索引
    /// （1–12 月收入 − 费用，与账套自洽）；亏损池更新仅在过账成功后落地。
    pub fn accrue_income_tax(
        &mut self,
        date: CivilDate,
    ) -> Result<IncomeTaxOutcome, IndustrialError> {
        let year = date.year();
        let pretax = self.year_pretax(year)?;
        let computation =
            compute_income_tax(pretax, year, &self.loss_pool, &self.tax_policy().income_tax)?;
        let posted_dta = self.net_of(chart::acct::DTA)?;
        let deferred_delta = computation.deferred_tax_asset.sub(posted_dta)?;

        let mut entries = Vec::new();
        let mut lines = Vec::new();
        if computation.current_tax.is_positive() {
            lines.push(super::line(
                chart::acct::TAX_EXP,
                PostingSide::Debit,
                computation.current_tax,
            ));
            lines.push(super::line(
                chart::acct::CIT_PAYABLE,
                PostingSide::Credit,
                computation.current_tax,
            ));
        }
        if deferred_delta.is_positive() {
            lines.push(super::line(
                chart::acct::DTA,
                PostingSide::Debit,
                deferred_delta,
            ));
            lines.push(super::line(
                chart::acct::TAX_EXP,
                PostingSide::Credit,
                deferred_delta,
            ));
        } else if deferred_delta.is_negative() {
            let release = deferred_delta.neg()?;
            lines.push(super::line(
                chart::acct::TAX_EXP,
                PostingSide::Debit,
                release,
            ));
            lines.push(super::line(chart::acct::DTA, PostingSide::Credit, release));
        }
        let event = if lines.is_empty() {
            // 零税前且无递延变动：无分录、无事件 id 消耗（ending_pool 与当前
            // 池一致，落地为幂等赋值）。
            None
        } else {
            let base = self.next_event_id;
            let event = BusinessEventId::new(base);
            entries.push(JournalEntry {
                source: event,
                date,
                kind: BusinessKind::TaxAccrual,
                cash_flow: CashFlowClass::NonCash,
                lines,
            });
            self.post_with_commit(base + 1, entries)?;
            Some(event)
        };
        self.loss_pool = computation.ending_pool.clone();
        Ok(IncomeTaxOutcome {
            event,
            pretax,
            current_tax: computation.current_tax,
            loss_offset_used: computation.loss_offset_used,
            loss_added: computation.loss_added,
            losses_expired: computation.losses_expired,
            deferred_delta,
        })
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

    /// 年度税前利润 = 全年（1–12 月）收入净贷方 − 费用净借方（总账期间索引派生）。
    fn year_pretax(&self, year: i32) -> Result<AccountingAmount, IndustrialError> {
        let mut revenue = AccountingAmount::ZERO;
        let mut expense = AccountingAmount::ZERO;
        for month in 1..=12u8 {
            let period = AccountingPeriod::from_ymd(year, month)?;
            for (id, def) in self.books().ledger().chart().iter() {
                let balance = self
                    .books()
                    .ledger()
                    .account_period_balance(id, period)
                    .net_debit()?;
                match def.element {
                    AccountElement::Revenue => revenue = revenue.sub(balance)?,
                    AccountElement::Expense => expense = expense.add(balance)?,
                    _ => {}
                }
            }
        }
        Ok(revenue.sub(expense)?)
    }
}
