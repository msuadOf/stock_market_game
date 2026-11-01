//! 送转共同批准的纯候选；股份、税务及 INFO 的安装由后续共同事务负责。

use super::capital_approval_context::CapitalApprovalContext;
use crate::account::StockCode;
use crate::accounting::{AccountingAmount, AccountingError};
use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use crate::company::share_registry::ShareRegistryError;
use crate::company::stock_distribution::{
    allocate_stock_distribution, StockDistributionBook, StockDistributionDeclaration,
    StockDistributionError, StockDistributionEventPlan, StockDistributionFinanceFact,
    StockDistributionPlan,
};
use crate::company::{CompanyId, CompanySystem, CompanySystemError};
use crate::session::SessionCorporateActions;
use crate::Money;

pub(crate) struct StockDistributionApprovalInput<'a> {
    pub system: &'a CompanySystem,
    pub actions: &'a SessionCorporateActions,
    pub calendar: &'a TradingCalendar,
    /// 当前 Session 时钟与该证券真实交易所，不能采用客户端自报上下文。
    pub day: CivilDate,
    pub exchange: CalendarExchange,
    pub plan: StockDistributionEventPlan,
    pub(super) capital: &'a CapitalApprovalContext,
}

#[derive(Debug)]
pub(crate) struct PreparedStockDistributionApproval {
    pub company_system: CompanySystem,
    pub actions: SessionCorporateActions,
    pub fact: StockDistributionFinanceFact,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum StockDistributionApprovalIssue {
    DateMismatch,
    ExchangeMismatch,
    UnknownIssuer,
    StockMismatch,
    ConflictingIdentity,
    ConflictingRegistrationIdentity,
    SameExDateDistribution,
    SameExDateRights,
    SameExDateSplit,
    MissingRegistry,
    DuplicateRegistry,
    RegistryMismatch,
    FutureRegistry,
    MissingLegalFacts,
    #[expect(
        dead_code,
        reason = "保留送转面值不可整除的业务错误分类；当前资本事实门禁只派生可整除面值，生产分类仍将该错误映射为 BusinessCondition"
    )]
    NonIntegralPar,
    NonPositiveOrOverflowPar,
    CapitalOverflow,
    MissingDeclaredFact,
    CapitalContextMismatch,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum StockDistributionApprovalCause {
    #[error("送转批准条件不满足：{0:?}")]
    Validation(StockDistributionApprovalIssue),
    #[error("{0}")]
    Company(#[from] CompanySystemError),
    #[error("{0}")]
    CapitalTransition(#[from] super::capital_command_candidates::CapitalCommandError),
    #[error("{0}")]
    Plan(#[from] StockDistributionError),
    #[error("{0}")]
    Registry(#[from] ShareRegistryError),
    #[error("{0}")]
    Accounting(#[from] AccountingError),
}

#[derive(Debug)]
pub(crate) struct StockDistributionApprovalError {
    pub company: CompanyId,
    pub stock: StockCode,
    pub event_id: String,
    pub day: CivilDate,
    pub cause: Box<StockDistributionApprovalCause>,
}

impl std::fmt::Display for StockDistributionApprovalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "送转候选失败：公司 {company}，证券 {stock:?}，事件 {event_id}，日期 {day}：{cause}",
            company = self.company,
            stock = self.stock,
            event_id = self.event_id,
            day = self.day,
            cause = self.cause
        )
    }
}

impl std::error::Error for StockDistributionApprovalError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.cause.as_ref())
    }
}

pub(crate) fn prepare_stock_distribution_approval(
    input: StockDistributionApprovalInput<'_>,
) -> Result<PreparedStockDistributionApproval, StockDistributionApprovalError> {
    let fail = |cause| StockDistributionApprovalError {
        company: input.plan.issuer,
        stock: input.plan.stock.clone(),
        event_id: input.plan.event_id.clone(),
        day: input.day,
        cause: Box::new(cause),
    };
    let invalid = |issue| fail(StockDistributionApprovalCause::Validation(issue));
    if input.capital.company() != input.plan.issuer
        || input.capital.code() != &input.plan.stock
        || input.capital.as_of() != input.day
        || !input.capital.matches_source(input.system)
    {
        return Err(invalid(
            StockDistributionApprovalIssue::CapitalContextMismatch,
        ));
    }
    input.plan.validate().map_err(|cause| fail(cause.into()))?;
    super::capital_command_candidates::validate_pending_split_before_credit(
        input.actions,
        input.plan.issuer,
        &input.plan.stock,
        &input.plan.event_id,
        input.plan.ex_rights_on,
    )
    .map_err(|cause| fail(cause.into()))?;
    if input.plan.approved_on > input.day || input.plan.announced_on < input.day {
        return Err(invalid(StockDistributionApprovalIssue::DateMismatch));
    }
    if input.plan.exchange != input.exchange {
        return Err(invalid(StockDistributionApprovalIssue::ExchangeMismatch));
    }
    let issuer = input
        .system
        .issuers()
        .get(&input.plan.issuer)
        .ok_or_else(|| invalid(StockDistributionApprovalIssue::UnknownIssuer))?;
    if issuer.listed_stock.as_ref() != Some(&input.plan.stock) {
        return Err(invalid(StockDistributionApprovalIssue::StockMismatch));
    }
    input
        .plan
        .validate_calendar(input.calendar)
        .map_err(|cause| fail(cause.into()))?;
    input
        .system
        .validate_stock_distribution_books(&input.actions.stock_distributions)
        .map_err(|cause| fail(cause.into()))?;
    if input
        .actions
        .stock_distributions
        .iter()
        .any(|book| book.plan().event_id == input.plan.event_id)
    {
        return Err(invalid(StockDistributionApprovalIssue::ConflictingIdentity));
    }
    let same_ex = |issuer: CompanyId, stock: &StockCode, day: CivilDate| {
        issuer == input.plan.issuer && stock == &input.plan.stock && day == input.plan.ex_rights_on
    };
    // 既有不支持的同日非现金组合只按具体种类拒绝；现金分红允许进入同组。
    if input.actions.stock_distributions.iter().any(|book| {
        let plan = book.plan();
        same_ex(plan.issuer, &plan.stock, plan.ex_rights_on)
    }) {
        return Err(invalid(
            StockDistributionApprovalIssue::SameExDateDistribution,
        ));
    }
    if input.actions.rights_offerings.iter().any(|book| {
        let plan = book.plan();
        same_ex(plan.issuer, &plan.stock, plan.ex_rights_on)
    }) {
        return Err(invalid(StockDistributionApprovalIssue::SameExDateRights));
    }
    if input.actions.share_splits.iter().any(|book| {
        let plan = book.plan();
        same_ex(plan.issuer, &plan.stock, plan.ex_rights_on)
    }) {
        return Err(invalid(StockDistributionApprovalIssue::SameExDateSplit));
    }
    let mut matching = input
        .actions
        .registries
        .iter()
        .filter(|registry| registry.stock() == &input.plan.stock);
    let registry = matching
        .next()
        .ok_or_else(|| invalid(StockDistributionApprovalIssue::MissingRegistry))?;
    if matching.next().is_some() {
        return Err(invalid(StockDistributionApprovalIssue::DuplicateRegistry));
    }
    registry.validate().map_err(|cause| fail(cause.into()))?;
    if registry.issuer() != &input.plan.issuer || registry.issued_shares() != issuer.issued_shares {
        return Err(invalid(StockDistributionApprovalIssue::RegistryMismatch));
    }
    if registry.settled_on() > input.day {
        return Err(invalid(StockDistributionApprovalIssue::FutureRegistry));
    }
    if registry.registration(&input.plan.event_id).is_some() {
        return Err(invalid(
            StockDistributionApprovalIssue::ConflictingRegistrationIdentity,
        ));
    }
    let legal = input
        .system
        .dividend_legal_facts(&input.plan.issuer)
        .map_err(|cause| fail(cause.into()))?
        .ok_or_else(|| invalid(StockDistributionApprovalIssue::MissingLegalFacts))?;
    if input.capital.registered_capital() != legal.registered_capital {
        return Err(invalid(
            StockDistributionApprovalIssue::CapitalContextMismatch,
        ));
    }
    let par = input.capital.par_value();
    if par <= Money::ZERO {
        return Err(invalid(
            StockDistributionApprovalIssue::NonPositiveOrOverflowPar,
        ));
    }
    let capital_increase = i128::from(par.cents())
        .checked_mul(i128::from(input.plan.approved_total_new_shares))
        .map(AccountingAmount::from_cents)
        .ok_or_else(|| invalid(StockDistributionApprovalIssue::CapitalOverflow))?;
    let declaration = StockDistributionDeclaration {
        event_id: input.plan.event_id.clone(),
        approval_reference: input.plan.approval_reference.clone(),
        kind: input.plan.kind.clone(),
        approved_on: input.plan.approved_on,
        new_shares: input.plan.approved_total_new_shares,
        par_value_per_share: par,
        capital_increase,
        registered_capital_at_approval: legal.registered_capital,
    };
    let mut company_system = input.system.clone();
    company_system
        .declare_stock_distribution(&input.plan.issuer, declaration)
        .map_err(|cause| fail(cause.into()))?;
    // 用当前已结算名册仅预检既有分配算法的可行域；私有快照不冒充未来 R 日
    // 登记，不安装分配结果。R 日真实份额变化仍须重新核验。
    let preview = registry
        .preview_current_snapshot(input.plan.event_id.clone())
        .map_err(|cause| fail(cause.into()))?;
    allocate_stock_distribution(
        &preview,
        &StockDistributionPlan {
            event_id: input.plan.event_id.clone(),
            approval_reference: input.plan.approval_reference.clone(),
            kind: input.plan.kind.clone(),
            shares_per_existing_share_micros: input.plan.shares_per_existing_share_micros,
            approved_total_new_shares: input.plan.approved_total_new_shares,
        },
        0,
    )
    .map_err(|cause| fail(cause.into()))?;
    let book =
        StockDistributionBook::new(input.plan.clone()).map_err(|cause| fail(cause.into()))?;
    let mut actions = input.actions.clone();
    actions.stock_distributions.push(book);
    actions
        .stock_distributions
        .sort_by(|left, right| left.plan().event_id.cmp(&right.plan().event_id));
    company_system
        .validate_stock_distribution_books(&actions.stock_distributions)
        .map_err(|cause| fail(cause.into()))?;
    let fact = company_system
        .stock_distribution_facts(&input.plan.issuer)
        .map_err(|cause| fail(cause.into()))?
        .into_iter()
        .find(|fact| fact.event_id == input.plan.event_id)
        .ok_or_else(|| invalid(StockDistributionApprovalIssue::MissingDeclaredFact))?;
    Ok(PreparedStockDistributionApproval {
        company_system,
        actions,
        fact,
    })
}
