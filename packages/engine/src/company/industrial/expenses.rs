//! 费用与税费（工商会计约束）：人员/管理/销售/研发费用（现金）、增值税净额结算、
//! 当期 + 递延所得税计提与缴纳。
//!
//! 增值税：应纳 = 销项（222101 贷方）− 可抵扣进项（222102 借方）；进项富余
//! 结转留抵（不模拟退税）。所得税：期间税前利润（收入 − 费用，按总账期间
//! 索引）→ 亏损 FIFO 弥补（池在过账成功后落地）→ 当期税；递延所得税资产
//! 差额计入所得税费用（全额确认简化，CAS 18 受阻 docs §2.1）。
//! 标签映射：缴税 = TaxPayment/Operating；所得税计提 = TaxAccrual/NonCash。

use crate::accounting::{
    AccountElement, AccountingAmount, AccountingPeriod, Books, BusinessEventId, BusinessKind,
    CashFlowClass, IncomeTaxComputation, IncomeTaxPolicy, JournalEntry, LedgerAccountId, LossEntry,
    PostingSide,
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
    pub current_tax_delta: AccountingAmount,
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
        let preview = self.income_tax_position.preview_income_tax(
            date.year(),
            self.books(),
            &self.tax_policy().income_tax,
        )?;
        let computation = preview.computation;
        let pretax = computation.pretax;
        let deferred_delta = preview.deferred_delta;
        let current_tax_delta = preview.current_tax_delta;

        let mut entries = Vec::new();
        let mut lines = Vec::new();
        if current_tax_delta.is_positive() {
            lines.push(super::line(
                chart::acct::TAX_EXP,
                PostingSide::Debit,
                current_tax_delta,
            ));
            lines.push(super::line(
                chart::acct::CIT_PAYABLE,
                PostingSide::Credit,
                current_tax_delta,
            ));
        } else if current_tax_delta.is_negative() {
            let reversal = current_tax_delta.neg()?;
            lines.push(super::line(
                chart::acct::CIT_PAYABLE,
                PostingSide::Debit,
                reversal,
            ));
            lines.push(super::line(
                chart::acct::TAX_EXP,
                PostingSide::Credit,
                reversal,
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
        self.income_tax_position.loss_pool = computation.ending_pool.clone();
        self.income_tax_position.assessment = Some(preview.assessment);
        Ok(IncomeTaxOutcome {
            event,
            pretax,
            current_tax: computation.current_tax,
            current_tax_delta,
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
}

/// 跨年亏损池与当前年度评估的唯一 owner；税款收付余额仍由 Books 持有。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IncomeTaxPosition {
    loss_pool: Vec<LossEntry>,
    #[serde(deserialize_with = "deserialize_assessment")]
    assessment: Option<IncomeTaxAssessment>,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct IncomeTaxAssessment {
    year: i32,
    pretax: AccountingAmount,
    current_tax: AccountingAmount,
    opening_loss_pool: Vec<LossEntry>,
}

fn deserialize_assessment<'de, Decoder: serde::Deserializer<'de>>(
    decoder: Decoder,
) -> Result<Option<IncomeTaxAssessment>, Decoder::Error> {
    serde::Deserialize::deserialize(decoder)
}

struct IncomeTaxPreview {
    computation: IncomeTaxComputation,
    deferred_delta: AccountingAmount,
    current_tax_delta: AccountingAmount,
    assessment: IncomeTaxAssessment,
}

impl IncomeTaxPosition {
    pub(super) fn loss_pool(&self) -> &[LossEntry] {
        &self.loss_pool
    }

    fn preview_income_tax(
        &self,
        year: i32,
        books: &Books,
        policy: &IncomeTaxPolicy,
    ) -> Result<IncomeTaxPreview, IndustrialError> {
        let pretax = self.year_pretax(year, books)?;
        if let Some(previous) = &self.assessment {
            if year < previous.year {
                return Err(IndustrialError::HistoricalTaxReassessmentUnsupported {
                    requested_year: year,
                    latest_year: previous.year,
                });
            }
        }
        let previous = self
            .assessment
            .as_ref()
            .filter(|previous| previous.year == year);
        let opening_loss_pool = match previous {
            Some(previous) => previous.opening_loss_pool.clone(),
            None => self.loss_pool.clone(),
        };
        if opening_loss_pool
            .iter()
            .any(|entry| entry.origin_year >= year)
        {
            return Err(IndustrialError::IncomeTaxStateInconsistent {
                detail: format!("{year} 年的年初亏损池不得包含本年或未来年份的亏损"),
            });
        }
        let computation = policy.compute(pretax, year, &opening_loss_pool)?;
        let previously_accrued = match previous {
            Some(previous) => previous.current_tax,
            None => AccountingAmount::ZERO,
        };
        let current_tax_delta = computation.current_tax.sub(previously_accrued)?;
        let posted_dta = books
            .ledger()
            .account_net_debit(&LedgerAccountId(chart::acct::DTA.to_string()))?;
        let deferred_delta = computation.deferred_tax_asset.sub(posted_dta)?;
        Ok(IncomeTaxPreview {
            assessment: IncomeTaxAssessment {
                year,
                pretax,
                current_tax: computation.current_tax,
                opening_loss_pool,
            },
            computation,
            deferred_delta,
            current_tax_delta,
        })
    }

    pub(super) fn validate(&self, policy: &IncomeTaxPolicy) -> Result<(), IndustrialError> {
        let invalid = |detail| IndustrialError::IncomeTaxStateInconsistent { detail };
        let validate_pool = |pool: &[LossEntry]| {
            for (index, entry) in pool.iter().enumerate() {
                if !entry.remaining.is_positive()
                    || !(crate::calendar::CIVIL_YEAR_MIN..=crate::calendar::CIVIL_YEAR_MAX)
                        .contains(&entry.origin_year)
                    || (index > 0 && pool[index - 1].origin_year >= entry.origin_year)
                {
                    return Err(invalid("亏损池必须按有效起源年严格递增且金额为正".into()));
                }
            }
            Ok(())
        };
        validate_pool(&self.loss_pool)?;
        if let Some(assessment) = &self.assessment {
            validate_pool(&assessment.opening_loss_pool)?;
            if !(crate::calendar::CIVIL_YEAR_MIN..=crate::calendar::CIVIL_YEAR_MAX)
                .contains(&assessment.year)
                || assessment
                    .opening_loss_pool
                    .iter()
                    .any(|entry| entry.origin_year >= assessment.year)
            {
                return Err(invalid("年度评估或年初亏损池的年份非法".into()));
            }
            let computation = policy.compute(
                assessment.pretax,
                assessment.year,
                &assessment.opening_loss_pool,
            )?;
            if computation.current_tax != assessment.current_tax
                || computation.ending_pool != self.loss_pool
            {
                return Err(invalid("年度评估税额或期末亏损池与年初基准不符".into()));
            }
        }
        Ok(())
    }

    /// 年度税前利润 = 全年（1–12 月）收入净贷方 − 费用净借方（总账期间索引派生）。
    fn year_pretax(&self, year: i32, books: &Books) -> Result<AccountingAmount, IndustrialError> {
        let mut revenue = AccountingAmount::ZERO;
        let mut expense = AccountingAmount::ZERO;
        for month in 1..=12u8 {
            let period = AccountingPeriod::from_ymd(year, month)?;
            for (id, def) in books.ledger().chart().iter() {
                let balance = books
                    .ledger()
                    .account_period_balance(id, period)
                    .net_debit()?;
                match def.element {
                    AccountElement::Revenue => revenue = revenue.sub(balance)?,
                    AccountElement::Expense if id.0 != chart::acct::TAX_EXP => {
                        expense = expense.add(balance)?
                    }
                    _ => {}
                }
            }
        }
        Ok(revenue.sub(expense)?)
    }
}
