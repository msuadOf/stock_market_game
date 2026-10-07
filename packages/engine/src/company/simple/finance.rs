pub use super::finance_config::{SimpleBookDisplayConfig, SimpleFinanceConfig, SimpleSummaryRule};
use crate::accounting::closing::{ClosingEngine, ClosingError};
use crate::accounting::consolidation::{MemberId, ScopeId};
use crate::accounting::reports::{IndustryPresentation, ReportError, ReportKind, ReportSet};
use crate::accounting::{
    AccountingAmount, AccountingPeriod, Books, BusinessEventId, BusinessKind, CashFlowClass,
    JournalEntry, JournalLine, LedgerAccountId, PostingSide, TaxPolicy, TaxPolicyError,
};
use crate::calendar::CivilDate;
use crate::company::api::PeriodAmounts;
use crate::company::dividend::{
    DistributableProfit, DividendDeclaration, DividendLegalFacts, DividendPaymentFact,
    DividendPaymentReceipt, DividendPlanFact, DividendPlanReceipt,
};
use crate::company::income_tax::{IncomeTaxOwnerError, IncomeTaxPosition};
use crate::company::stock_distribution::{
    StockDistributionDeclaration, StockDistributionFinanceFact,
};
use crate::company::{CompanyId, CompanyKind};
#[path = "finance_dividend.rs"]
mod dividend;
#[path = "finance_kind.rs"]
mod kind;
#[path = "finance_posting.rs"]
mod posting;
#[path = "finance_report_validation.rs"]
mod report_validation;
#[path = "finance_rights_offering.rs"]
mod rights_offering;
#[path = "finance_issuer_repurchase.rs"]
mod issuer_repurchase;
#[path = "finance_state.rs"]
mod state;
#[path = "finance_stock_distribution.rs"]
mod stock_distribution;
#[cfg(test)]
#[path = "finance_stock_distribution_tests.rs"]
mod stock_distribution_tests;
#[path = "finance_share_split.rs"]
mod share_split;
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
    #[error("分红计划标识已被不同内容占用：{0}")]
    DividendPlanConflict(String),
    #[error("分红法定事实已被不同内容占用：{0}")]
    DividendLegalFactsConflict(String),
    #[error("Simple 分红暂不支持：{0}")]
    DividendUnsupported(String),
    #[error("Simple 分红状态非法：{0}")]
    DividendInvalid(String),
    #[error("Simple 送转状态非法：{0}")]
    StockDistributionInvalid(String),
    #[error("Simple 送转事件标识已被不同内容占用：{0}")]
    StockDistributionConflict(String),
    #[error("Simple 拆股／缩股状态非法：{0}")]
    ShareSplitInvalid(String),
    #[error("Simple 拆股／缩股事件标识已被不同内容占用：{0}")]
    ShareSplitConflict(String),
}

impl SimpleFinanceError {
    /// 四分类附加面（委托给 [`crate::company::CompanySystemError::classification`]）：
    /// 状态机阶段拒绝与事实冲突属业务条件拒绝，显式「暂不支持」属未支持操作，
    /// 底层会计／封账／报表／税务错误属系统状态错误，非法日期属非法输入。
    pub fn classification(&self) -> crate::company::error::CompanyErrorClass {
        use crate::company::error::CompanyErrorClass;
        match self {
            Self::Invalid(_) | Self::Date(_) => CompanyErrorClass::InvalidInput,
            Self::DividendUnsupported(_) => CompanyErrorClass::UnsupportedOperation,
            Self::DividendPlanConflict(_)
            | Self::DividendLegalFactsConflict(_)
            | Self::DividendInvalid(_)
            | Self::StockDistributionInvalid(_)
            | Self::StockDistributionConflict(_)
            | Self::ShareSplitInvalid(_)
            | Self::ShareSplitConflict(_) => CompanyErrorClass::BusinessCondition,
            Self::Accounting(_)
            | Self::Closing(_)
            | Self::Report(_)
            | Self::Policy(_)
            | Self::Tax(_) => CompanyErrorClass::SystemState,
        }
    }
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DividendPlanState {
    declaration: DividendDeclaration,
    declaration_source: BusinessEventId,
    reserve: AccountingAmount,
    #[serde(deserialize_with = "deserialize_required_option")]
    reserve_basis_year: RequiredOption<i32>,
    payments: std::collections::BTreeMap<String, DividendPaymentState>,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct DividendPaymentState {
    source: BusinessEventId,
    paid_on: CivilDate,
    amount: AccountingAmount,
}

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct RequiredOption<T>(Option<T>);

fn deserialize_required_option<'de, T, D>(deserializer: D) -> Result<RequiredOption<T>, D::Error>
where
    T: serde::Deserialize<'de>,
    D: serde::Deserializer<'de>,
{
    serde::Deserialize::deserialize(deserializer)
}

/// Simple 账面展示字段快照（查询投影）。两字段均为**账面展示值**：
/// cash_book = 累计净利润 − 累计已付分红（留存收益口径）、investment_book =
/// 累计收入 × 配置比例；不代表真实公司资金，不参与任何资金结算
///（2026-10-08 用户决策，账面/真实分离铁律）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, ts_rs::TS)]
#[ts(export)]
pub struct SimpleBookDisplay {
    #[ts(type = "string")]
    pub cash_book: AccountingAmount,
    #[ts(type = "string")]
    pub investment_book: AccountingAmount,
    pub investment_of_revenue_bp: i32,
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
    dividends: std::collections::BTreeMap<String, DividendPlanState>,
    stock_distributions:
        std::collections::BTreeMap<String, StockDistributionFinanceFact>,
    share_splits:
        std::collections::BTreeMap<String, crate::company::share_split::ShareSplitFinanceFact>,
    rights_offerings: std::collections::BTreeMap<
        String,
        crate::company::rights_offering::RightsOfferingFinanceFact,
    >,
    issuer_repurchases:
        std::collections::BTreeMap<String, crate::company::issuer_repurchase::IssuerRepurchaseFinanceFact>,
    #[serde(deserialize_with = "deserialize_required_option")]
    legal_facts: RequiredOption<DividendLegalFacts>,
    /// 账面展示现金（2026-10-08 用户决策）：累计净利润 − 累计已付分红（留存收益
    /// 口径）。每结算周期末与分红付款后重算；仅为展示值，不代表真实资金，
    /// 不参与任何资金结算。严格持久化字段，旧档缺失显式拒绝。
    cash_book: AccountingAmount,
    /// 账面展示投资额：累计收入 × 配置比例（`book_display` 半偶舍入落分）。
    /// 同上仅为展示值；严格持久化字段。
    investment_book: AccountingAmount,
}
impl SimpleFinanceState {
    pub fn legal_facts(&self) -> &Option<DividendLegalFacts> {
        &self.legal_facts.0
    }

    /// 账面展示现金：累计净利润 − 累计已付分红（留存收益口径）。
    pub fn cash_book(&self) -> AccountingAmount {
        self.cash_book
    }

    /// 账面展示投资额：累计收入 × 配置比例。
    pub fn investment_book(&self) -> AccountingAmount {
        self.investment_book
    }

    /// 账面展示字段查询快照（含配置比例；均为展示值，非真实资金）。
    pub fn book_display(&self) -> SimpleBookDisplay {
        SimpleBookDisplay {
            cash_book: self.cash_book,
            investment_book: self.investment_book,
            investment_of_revenue_bp: self.config.book_display.investment_of_revenue_bp,
        }
    }

    /// 从权威账簿与分红付款事实重算两个账面展示字段（不变量口径）。
    fn refresh_book_display(&mut self) -> Result<(), SimpleFinanceError> {
        let mut paid_dividends = AccountingAmount::ZERO;
        for plan in self.dividends.values() {
            for payment in plan.payments.values() {
                paid_dividends = paid_dividends.add(payment.amount)?;
            }
        }
        self.cash_book = self.books.ledger().net_income()?.sub(paid_dividends)?;
        let cumulative_revenue = self
            .books
            .ledger()
            .account_net_debit(&LedgerAccountId(
                crate::accounting::reports::simple_summary::REVENUE.into(),
            ))?
            .neg()?;
        self.investment_book = cumulative_revenue
            .apply_basis_points(self.config.book_display.investment_of_revenue_bp)?;
        Ok(())
    }

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
        candidate.refresh_book_display()?;
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
#[path = "finance_dividend_tests.rs"]
mod dividend_tests;
#[cfg(test)]
#[path = "finance_books_integration_tests.rs"]
mod books_integration_tests;
#[cfg(test)]
#[path = "finance_kind_tests.rs"]
mod kind_tests;
#[cfg(test)]
#[path = "finance_period_tests.rs"]
mod period_tests;
#[cfg(test)]
#[path = "finance_tests.rs"]
mod tests;
