pub use super::finance_config::{SimpleFinanceConfig, SimpleSummaryRule};
use crate::accounting::closing::{ClosingEngine, ClosingError};
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::{IndustryPresentation, ReportError, ReportKind, ReportSet};
use crate::accounting::{
    AccountingAmount, AccountingPeriod, Books, BusinessEventId, BusinessKind, CashFlowClass,
    JournalEntry, JournalLine, LedgerAccountId, PostingSide, TaxPolicy, TaxPolicyError,
};
use crate::calendar::CivilDate;
use crate::company::api::PeriodAmounts;
use crate::company::income_tax::{IncomeTaxOwnerError, IncomeTaxPosition};
use crate::company::{CompanyId, CompanyKind};
#[path = "finance_kind.rs"]
mod kind;
#[path = "finance_posting.rs"]
mod posting;
#[path = "finance_report_validation.rs"]
mod report_validation;
#[path = "finance_state.rs"]
mod state;
#[path = "finance_validation.rs"]
mod validation;

#[derive(Debug, thiserror::Error)]
pub enum SimpleFinanceError {
    #[error("Simple 汇总财务不合法：{0}")]
    Invalid(String),
    #[error(transparent)]
    Accounting(#[from] crate::accounting::AccountingError),
    #[error(transparent)]
    Closing(#[from] ClosingError),
    #[error(transparent)]
    Report(#[from] ReportError),
    #[error(transparent)]
    Policy(#[from] TaxPolicyError),
    #[error(transparent)]
    Tax(#[from] IncomeTaxOwnerError),
    #[error(transparent)]
    Date(#[from] crate::calendar::CivilDateError),
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SimpleFinanceState {
    company: CompanyId,
    kind: CompanyKind,
    config: SimpleFinanceConfig,
    books: Books,
    closing: ClosingEngine,
    opening_date: CivilDate,
    as_of: CivilDate,
    last_month: AccountingPeriod,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    next_event_id: u64,
    income_tax_position: IncomeTaxPosition,
    recognized_periods: Vec<(CivilDate, CivilDate)>,
}
impl SimpleFinanceState {
    pub fn apply_month(
        &mut self,
        date: CivilDate,
        amounts: &PeriodAmounts,
    ) -> Result<(), SimpleFinanceError> {
        self.apply_period(
            CivilDate::from_ymd(date.year(), date.month(), 1)?,
            date,
            amounts,
        )
    }
    pub fn apply_period(
        &mut self,
        start: CivilDate,
        end: CivilDate,
        amounts: &PeriodAmounts,
    ) -> Result<(), SimpleFinanceError> {
        if start != self.as_of.next()?
            || start.day() != 1
            || end < start
            || end.next()?.month() == end.month()
            || start.year() != end.year()
        {
            return Err(SimpleFinanceError::Invalid(
                "摘要期间须连续、完整并处于同一自然年度".into(),
            ));
        }
        if [
            amounts.revenue,
            amounts.fixed_expense,
            amounts.variable_expense,
        ]
        .iter()
        .any(|amount| amount.is_negative())
        {
            return Err(SimpleFinanceError::Invalid(
                "收入及费用生成金额不得为负".into(),
            ));
        }
        let mut candidate = self.clone();
        candidate.post_summary(end, amounts)?;
        candidate.accrue_tax(end)?;
        candidate.recognized_periods.push((start, end));
        candidate.as_of = end;
        candidate.last_month = AccountingPeriod::of_date(end);
        candidate.close_generated_period(start, end)?;
        candidate.validate()?;
        *self = candidate;
        Ok(())
    }
}
fn line(account: &str, side: PostingSide, amount: AccountingAmount) -> JournalLine {
    JournalLine {
        account: LedgerAccountId(account.into()),
        side,
        amount,
    }
}
#[cfg(test)]
#[path = "finance_kind_tests.rs"]
mod kind_tests;
#[cfg(test)]
#[path = "finance_period_tests.rs"]
mod period_tests;
#[cfg(test)]
#[path = "finance_tests.rs"]
mod tests;
