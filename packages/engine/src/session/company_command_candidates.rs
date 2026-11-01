//! 公司命令的纯候选；结果必须由共同事务安装，不是对外受理回执。

use crate::account::StockCode;
use crate::accounting::{AccountingAmount, AccountingError};
use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use crate::company::cash_dividend::{CashDividendBook, CashDividendError, CashDividendPlan};
use crate::company::ex_reference_price::CashDividendFormula;
use crate::company::share_registry::{HolderId, ShareRegistryError};
use crate::company::{
    CompanyId, CompanySystem, CompanySystemError, DividendDeclaration, DividendPlanReceipt,
};
use crate::session::SessionCorporateActions;
use crate::Money;

/// day 与 exchange 必须取当前 Session 时钟及证券规格；不能由提案自报覆盖。
pub(crate) struct CashDividendApprovalInput<'a> {
    pub system: &'a CompanySystem,
    pub actions: &'a SessionCorporateActions,
    pub calendar: &'a TradingCalendar,
    pub day: CivilDate,
    pub exchange: CalendarExchange,
    pub plan: CashDividendPlan,
}

/// 只有私有公司与账簿候选；不含投资者付款、INFO 安装或独立 commit 能力。
#[derive(Debug)]
pub(crate) struct PreparedCashDividendApproval {
    pub company_system: CompanySystem,
    pub actions: SessionCorporateActions,
    pub receipt: DividendPlanReceipt,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum CashDividendApprovalIssue {
    DateMismatch,
    ExchangeMismatch,
    UnknownIssuer,
    StockMismatch,
    MissingRegistry,
    DuplicateRegistry,
    RegistryMismatch,
    FutureRegistry,
    NoEligibleShares,
    GrossOverflow,
    MissingLegalFacts,
    AmountExceedsPlan,
    UnsupportedFormula,
    ConflictingIdentity,
    UnsupportedRightsCombination,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum CompanyCommandCause {
    #[error("现金分红批准条件不满足：{0:?}")]
    Validation(CashDividendApprovalIssue),
    #[error("{0}")]
    Company(#[from] CompanySystemError),
    #[error("{0}")]
    Plan(#[from] CashDividendError),
    #[error("{0}")]
    Registry(#[from] ShareRegistryError),
    #[error("{0}")]
    Accounting(#[from] AccountingError),
    #[error("{0}")]
    Money(#[from] crate::MoneyError),
}

/// Rust 内部错误保留原 typed source；宿主数值错误转换仍属后续独立边界。
#[derive(Debug)]
pub(crate) struct CompanyCommandError {
    pub company: CompanyId,
    pub stock: StockCode,
    pub plan_id: String,
    pub day: CivilDate,
    pub cause: Box<CompanyCommandCause>,
}

impl std::fmt::Display for CompanyCommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "现金分红候选失败：公司 {company}，证券 {stock:?}，计划 {plan_id}，日期 {day}：{cause}",
            company = self.company,
            stock = self.stock,
            plan_id = self.plan_id,
            day = self.day,
            cause = self.cause
        )
    }
}

impl std::error::Error for CompanyCommandError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        // 返回原 cause，避免把 Box 本身变成 source 的动态类型。
        Some(self.cause.as_ref())
    }
}

pub(crate) fn prepare_cash_dividend_approval(
    input: CashDividendApprovalInput<'_>,
) -> Result<PreparedCashDividendApproval, CompanyCommandError> {
    let fail = |cause| CompanyCommandError {
        company: input.plan.issuer,
        stock: input.plan.stock.clone(),
        plan_id: input.plan.plan_id.clone(),
        day: input.day,
        cause: Box::new(cause),
    };
    let invalid = |issue| fail(CompanyCommandCause::Validation(issue));
    input.plan.validate().map_err(|cause| fail(cause.into()))?;
    if input.plan.approved_on > input.day || input.plan.announced_on < input.day {
        return Err(invalid(CashDividendApprovalIssue::DateMismatch));
    }
    if input.plan.exchange != input.exchange {
        return Err(invalid(CashDividendApprovalIssue::ExchangeMismatch));
    }
    let issuer = input
        .system
        .issuers()
        .get(&input.plan.issuer)
        .ok_or_else(|| invalid(CashDividendApprovalIssue::UnknownIssuer))?;
    if issuer.listed_stock.as_ref() != Some(&input.plan.stock) {
        return Err(invalid(CashDividendApprovalIssue::StockMismatch));
    }
    input
        .plan
        .validate_calendar(input.calendar)
        .map_err(|cause| fail(cause.into()))?;
    if input.plan.formula == CashDividendFormula::ExchangeApprovedAdjustment {
        return Err(invalid(CashDividendApprovalIssue::UnsupportedFormula));
    }
    // 先拒绝基础账簿撕裂，不能用新审批覆盖或补全已有坏状态。
    input
        .system
        .validate_cash_dividend_books(&input.actions.dividends)
        .map_err(|cause| fail(cause.into()))?;
    if input
        .actions
        .dividends
        .iter()
        .any(|book| book.plan().plan_id == input.plan.plan_id)
    {
        return Err(invalid(CashDividendApprovalIssue::ConflictingIdentity));
    }
    let mut matching = input
        .actions
        .registries
        .iter()
        .filter(|registry| registry.stock() == &input.plan.stock);
    let registry = matching
        .next()
        .ok_or_else(|| invalid(CashDividendApprovalIssue::MissingRegistry))?;
    if matching.next().is_some() {
        return Err(invalid(CashDividendApprovalIssue::DuplicateRegistry));
    }
    registry.validate().map_err(|cause| fail(cause.into()))?;
    if registry.issuer() != &input.plan.issuer || registry.issued_shares() != issuer.issued_shares {
        return Err(invalid(CashDividendApprovalIssue::RegistryMismatch));
    }
    if registry.settled_on() > input.day {
        return Err(invalid(CashDividendApprovalIssue::FutureRegistry));
    }
    let total_gross = registry
        .holdings()
        .iter()
        .filter(|holding| holding.holder != HolderId::IssuerTreasury)
        .flat_map(|holding| &holding.lots)
        .try_fold(0_i128, |total, lot| {
            i128::from(input.plan.gross_per_share.cents())
                .checked_mul(i128::from(lot.qty))
                .and_then(|amount| total.checked_add(amount))
        })
        .and_then(|amount| i64::try_from(amount).ok())
        .map(Money::from_cents)
        .ok_or_else(|| invalid(CashDividendApprovalIssue::GrossOverflow))?;
    if super::capital_command_candidates::has_rights_ex_date_conflict(
        input.actions,
        input.plan.issuer,
        &input.plan.stock,
        input.plan.ex_dividend_on,
    ) {
        return Err(invalid(
            CashDividendApprovalIssue::UnsupportedRightsCombination,
        ));
    }
    if total_gross == Money::ZERO {
        return Err(invalid(CashDividendApprovalIssue::NoEligibleShares));
    }
    if total_gross > input.plan.distributable_amount {
        return Err(invalid(CashDividendApprovalIssue::AmountExceedsPlan));
    }
    let legal = input
        .system
        .dividend_legal_facts(&input.plan.issuer)
        .map_err(|cause| fail(cause.into()))?
        .ok_or_else(|| invalid(CashDividendApprovalIssue::MissingLegalFacts))?;
    let _same_ex_date_gross_per_share = input
        .actions
        .dividends
        .iter()
        .filter(|book| {
            book.plan().stock == input.plan.stock
                && book.plan().ex_dividend_on == input.plan.ex_dividend_on
        })
        .try_fold(input.plan.gross_per_share, |total, book| {
            total.add(book.plan().gross_per_share)
        })
        .map_err(|cause| fail(cause.into()))?;
    let declaration = DividendDeclaration {
        plan_id: input.plan.plan_id.clone(),
        approved_on: input.plan.approved_on,
        total_gross: AccountingAmount::from_money(total_gross),
        registered_capital: legal.registered_capital,
    };
    let mut company_system = input.system.clone();
    // 唯一模型方法承担累计可分配利润与法定公积金检查；不复制其公式。
    let receipt = company_system
        .declare_dividend(&input.plan.issuer, declaration)
        .map_err(|cause| fail(cause.into()))?;
    let book = CashDividendBook::new(input.plan.clone()).map_err(|cause| fail(cause.into()))?;
    let mut actions = input.actions.clone();
    actions.dividends.push(book);
    actions
        .dividends
        .sort_by(|left, right| left.plan().plan_id.cmp(&right.plan().plan_id));
    company_system
        .validate_cash_dividend_books(&actions.dividends)
        .map_err(|cause| fail(cause.into()))?;
    Ok(PreparedCashDividendApproval {
        company_system,
        actions,
        receipt,
    })
}
