//! 配股／增发与拆缩股共同批准；只准备候选，真实缴款和股份换算由后续相位执行。

use super::capital_approval_context::CapitalApprovalContext;
use super::SessionCorporateActions;
use crate::calendar::{CalendarExchange, CivilDate, TradingCalendar};
use crate::company::rights_offering::{
    RightsOfferingBook, RightsOfferingDeclaration, RightsOfferingError, RightsOfferingEventPlan,
};
use crate::company::share_split::{
    ShareSplitBook, ShareSplitDeclaration, ShareSplitDirection, ShareSplitError,
    ShareSplitEventPlan,
};
use crate::company::{CompanyId, CompanySystem, CompanySystemError};
use crate::{Money, StockCode};

pub(super) struct CapitalCommandInput<'a> {
    pub system: &'a CompanySystem,
    pub actions: &'a SessionCorporateActions,
    pub calendar: &'a TradingCalendar,
    pub day: CivilDate,
    pub exchange: CalendarExchange,
    pub capital: &'a CapitalApprovalContext,
}

pub(super) struct PreparedCapitalApproval {
    pub company_system: CompanySystem,
    pub actions: SessionCorporateActions,
    pub event_id: String,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum CapitalCommandError {
    #[error("资本批准方案与当前来源、日期、发行人或证券不一致")]
    ContextMismatch,
    #[error("公司 {company} 的事件身份 {event_id} 已存在")]
    Duplicate {
        company: CompanyId,
        event_id: String,
    },
    #[error("公司 {company} 证券 {stock:?} 在 {ex_on} 存在未支持合并的非现金除权事件")]
    SameExDate {
        company: CompanyId,
        stock: StockCode,
        ex_on: CivilDate,
    },
    #[error("公司 {company} 的拆股面值 {par:?} 不能按整数比例 {ratio} 换算为正整数分")]
    InvalidSplitPar {
        company: CompanyId,
        par: Money,
        ratio: u64,
    },
    #[error("配股／增发批准失败：{0}")]
    Rights(#[from] RightsOfferingError),
    #[error("拆股／缩股批准失败：{0}")]
    Split(#[from] ShareSplitError),
    #[error("共同公司批准失败：{0}")]
    Company(#[from] CompanySystemError),
    #[error("配股能力校验失败：{0}")]
    Capability(
        #[from] super::corporate_actions::rights_offering_day_end::RightsOfferingCandidateError,
    ),
    #[error("证券 {stock:?} 的待生效重新计值 {split_event}（登记 {split_registered_on}，除权 {split_on}）与资本动作 {capital_event}（入账 {capital_on}）缺少跨单位或登记相位执行来源")]
    PendingShareUnitTransition {
        stock: StockCode,
        split_event: String,
        split_registered_on: CivilDate,
        split_on: CivilDate,
        capital_event: String,
        capital_on: CivilDate,
    },
}

fn validate_context(
    input: &CapitalCommandInput<'_>,
    company: CompanyId,
    stock: &StockCode,
    approved_on: CivilDate,
    announced_on: CivilDate,
    exchange: CalendarExchange,
) -> Result<(), CapitalCommandError> {
    if company != input.capital.company()
        || stock != input.capital.code()
        || input.capital.as_of() != input.day
        || !input.capital.matches_source(input.system)
        || approved_on != input.day
        || announced_on != input.day
        || exchange != input.exchange
    {
        return Err(CapitalCommandError::ContextMismatch);
    }
    Ok(())
}

pub(super) fn has_rights_ex_date_conflict(
    actions: &SessionCorporateActions,
    company: CompanyId,
    stock: &StockCode,
    ex_on: CivilDate,
) -> bool {
    actions.rights_offerings.iter().any(|book| {
        let plan = book.plan();
        plan.issuer == company && &plan.stock == stock && plan.ex_rights_on == ex_on
    })
}

fn validate_ex_date(
    input: &CapitalCommandInput<'_>,
    company: CompanyId,
    stock: &StockCode,
    ex_on: CivilDate,
) -> Result<(), CapitalCommandError> {
    let same = |issuer: CompanyId, code: &StockCode, day: CivilDate| {
        issuer == company && code == stock && day == ex_on
    };
    if input.actions.stock_distributions.iter().any(|book| {
        let plan = book.plan();
        same(plan.issuer, &plan.stock, plan.ex_rights_on)
    }) || has_rights_ex_date_conflict(input.actions, company, stock, ex_on)
        || input.actions.share_splits.iter().any(|book| {
            let plan = book.plan();
            same(plan.issuer, &plan.stock, plan.ex_rights_on)
        })
    {
        return Err(CapitalCommandError::SameExDate {
            company,
            stock: stock.clone(),
            ex_on,
        });
    }
    Ok(())
}

fn validate_rights_ex_date(
    input: &CapitalCommandInput<'_>,
    company: CompanyId,
    stock: &StockCode,
    ex_on: CivilDate,
) -> Result<(), CapitalCommandError> {
    validate_ex_date(input, company, stock, ex_on)?;
    if input.actions.dividends.iter().any(|book| {
        let plan = book.plan();
        plan.issuer == company && &plan.stock == stock && plan.ex_dividend_on == ex_on
    }) {
        return Err(CapitalCommandError::SameExDate {
            company,
            stock: stock.clone(),
            ex_on,
        });
    }
    Ok(())
}

/// 已批准的拆缩股尚未真正入账时，登记当日或后续发行缺少精确相位来源。
/// 登记前完成的独立资本动作不受此门影响；不同 issuer/stock 也不受影响。
pub(super) fn validate_pending_split_before_credit(
    actions: &SessionCorporateActions,
    company: CompanyId,
    stock: &StockCode,
    event_id: &str,
    credited_on: CivilDate,
) -> Result<(), CapitalCommandError> {
    for book in &actions.share_splits {
        let split = book.plan();
        if book.settled_on().is_none()
            && split.issuer == company
            && &split.stock == stock
            && split.registered_on <= credited_on
        {
            return Err(CapitalCommandError::PendingShareUnitTransition {
                stock: stock.clone(),
                split_event: split.event_id.clone(),
                split_registered_on: split.registered_on,
                split_on: split.ex_rights_on,
                capital_event: event_id.to_owned(),
                capital_on: credited_on,
            });
        }
    }
    Ok(())
}

fn validate_new_split_before_pending_credit(
    actions: &SessionCorporateActions,
    plan: &ShareSplitEventPlan,
) -> Result<(), CapitalCommandError> {
    for book in &actions.stock_distributions {
        let credit = book.plan();
        if book.credited_on().is_none()
            && credit.issuer == plan.issuer
            && credit.stock == plan.stock
            && plan.registered_on <= credit.ex_rights_on
        {
            return Err(CapitalCommandError::PendingShareUnitTransition {
                stock: plan.stock.clone(),
                split_event: plan.event_id.clone(),
                split_registered_on: plan.registered_on,
                split_on: plan.ex_rights_on,
                capital_event: credit.event_id.clone(),
                capital_on: credit.ex_rights_on,
            });
        }
    }
    for book in &actions.rights_offerings {
        let credit = book.plan();
        if book.credited_on().is_none()
            && !book
                .settlement()
                .is_some_and(|settlement| settlement.failed)
            && credit.issuer == plan.issuer
            && credit.stock == plan.stock
            && plan.registered_on <= credit.listing_on
        {
            return Err(CapitalCommandError::PendingShareUnitTransition {
                stock: plan.stock.clone(),
                split_event: plan.event_id.clone(),
                split_registered_on: plan.registered_on,
                split_on: plan.ex_rights_on,
                capital_event: credit.event_id.clone(),
                capital_on: credit.listing_on,
            });
        }
    }
    Ok(())
}

pub(super) fn prepare_rights_approval(
    input: CapitalCommandInput<'_>,
    plan: RightsOfferingEventPlan,
) -> Result<PreparedCapitalApproval, CapitalCommandError> {
    plan.validate()?;
    plan.validate_calendar(input.calendar)?;
    super::corporate_actions::rights_offering_day_end::validate_rights_offering_capability(&plan)?;
    validate_context(
        &input,
        plan.issuer,
        &plan.stock,
        plan.approved_on,
        plan.announced_on,
        plan.exchange,
    )?;
    input
        .system
        .validate_rights_offering_books(&input.actions.rights_offerings)?;
    if input
        .actions
        .rights_offerings
        .iter()
        .any(|book| book.plan().event_id == plan.event_id)
    {
        return Err(CapitalCommandError::Duplicate {
            company: plan.issuer,
            event_id: plan.event_id,
        });
    }
    validate_rights_ex_date(&input, plan.issuer, &plan.stock, plan.ex_rights_on)?;
    validate_pending_split_before_credit(
        input.actions,
        plan.issuer,
        &plan.stock,
        &plan.event_id,
        plan.listing_on,
    )?;
    let mut company_system = input.system.clone();
    company_system.declare_rights_offering(
        &plan.issuer,
        RightsOfferingDeclaration {
            event_id: plan.event_id.clone(),
            approval_reference: plan.approval_reference.clone(),
            approved_on: plan.approved_on,
            price_per_share: plan.price_per_share,
            par_value_per_share: input.capital.par_value(),
            registered_capital_at_approval: input.capital.registered_capital(),
        },
    )?;
    let event_id = plan.event_id.clone();
    let mut actions = input.actions.clone();
    actions
        .rights_offerings
        .push(RightsOfferingBook::new(plan)?);
    actions
        .rights_offerings
        .sort_by(|left, right| left.plan().event_id.cmp(&right.plan().event_id));
    company_system.validate_rights_offering_books(&actions.rights_offerings)?;
    Ok(PreparedCapitalApproval {
        company_system,
        actions,
        event_id,
    })
}

pub(super) fn prepare_split_approval(
    input: CapitalCommandInput<'_>,
    plan: ShareSplitEventPlan,
) -> Result<PreparedCapitalApproval, CapitalCommandError> {
    plan.validate()?;
    plan.validate_calendar(input.calendar)?;
    validate_context(
        &input,
        plan.issuer,
        &plan.stock,
        plan.approved_on,
        plan.announced_on,
        plan.exchange,
    )?;
    input
        .system
        .validate_share_split_books(&input.actions.share_splits)?;
    if input
        .actions
        .share_splits
        .iter()
        .any(|book| book.plan().event_id == plan.event_id)
    {
        return Err(CapitalCommandError::Duplicate {
            company: plan.issuer,
            event_id: plan.event_id,
        });
    }
    validate_ex_date(&input, plan.issuer, &plan.stock, plan.ex_rights_on)?;
    validate_new_split_before_pending_credit(input.actions, &plan)?;
    let before = input.capital.par_value();
    let invalid = || CapitalCommandError::InvalidSplitPar {
        company: plan.issuer,
        par: before,
        ratio: plan.ratio,
    };
    let after = match plan.direction {
        ShareSplitDirection::Split => {
            if u128::try_from(before.cents()).map_err(|_| invalid())? % u128::from(plan.ratio) != 0
            {
                return Err(invalid());
            }
            i128::from(before.cents()).checked_div(i128::from(plan.ratio))
        }
        ShareSplitDirection::Consolidate => {
            i128::from(before.cents()).checked_mul(i128::from(plan.ratio))
        }
    }
    .and_then(|value| i64::try_from(value).ok())
    .filter(|value| *value > 0)
    .ok_or_else(invalid)?;
    let mut company_system = input.system.clone();
    company_system.declare_share_split(
        &plan.issuer,
        ShareSplitDeclaration {
            event_id: plan.event_id.clone(),
            approval_reference: plan.approval_reference.clone(),
            direction: plan.direction.clone(),
            approved_on: plan.approved_on,
            ratio: plan.ratio,
            par_value_before: before,
            par_value_after: Money::from_cents(after),
            registered_capital_at_approval: input.capital.registered_capital(),
        },
    )?;
    let event_id = plan.event_id.clone();
    let mut actions = input.actions.clone();
    actions.share_splits.push(ShareSplitBook::new(plan)?);
    actions
        .share_splits
        .sort_by(|left, right| left.plan().event_id.cmp(&right.plan().event_id));
    company_system.validate_share_split_books(&actions.share_splits)?;
    Ok(PreparedCapitalApproval {
        company_system,
        actions,
        event_id,
    })
}
