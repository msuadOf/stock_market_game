//! ADR-0037 的持仓玩家提案简化；仅准备候选，不提交公司或投资者状态。

use super::company_command_candidates::{
    prepare_cash_dividend_approval, CashDividendApprovalInput, CompanyCommandError,
    PreparedCashDividendApproval,
};
use super::company_commands_types::{PlayerCompanyProposal, PlayerProposalKind};
use super::stock_distribution_command_candidate::{
    prepare_stock_distribution_approval, PreparedStockDistributionApproval,
    StockDistributionApprovalError, StockDistributionApprovalInput,
};
use crate::account::StockCode;
use crate::accounting::AccountingError;
use crate::calendar::{CalendarError, CalendarExchange, CivilDate, TradingCalendar};
use crate::company::cash_dividend::{CashDividendError, CashDividendPlan};
use crate::company::ex_reference_price::CashDividendFormula;
use crate::company::rights_offering::{
    DirectedPlacementTarget, RightsOfferingError, RightsOfferingEventPlan, RightsOfferingMode,
    RightsSubscriptionStrategy,
};
use crate::company::{CompanyId, CompanySystem, CompanySystemError};
use crate::session::{SessionCorporateActions, StockExchange, StockSpec};
use crate::{Account, AccountId, Money, StockCatalog, StockId};

/// requester、security、day、calendar 均须来自当前 Session，不接受提案自报。
pub(crate) struct PlayerCompanyCommandInput<'a> {
    pub requester: &'a Account,
    pub catalog: &'a StockCatalog,
    pub security: &'a StockSpec,
    pub system: &'a CompanySystem,
    pub actions: &'a SessionCorporateActions,
    pub calendar: &'a TradingCalendar,
    pub day: CivilDate,
    pub proposal: PlayerCompanyProposal,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum PlayerCommandIssue {
    UnexpectedProposalKind,
    UnknownIssuer,
    NotListed,
    UnknownStock,
    StockMismatch,
    NonPositiveGross,
    NonPositiveRatio,
    MissingRegistry,
    ShareCountOverflow,
    NoNewShares,
    RightsOfferingDisabled,
    NonPositiveRepurchaseWindow,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum PlayerCommandCause {
    #[error("玩家提案前置条件不满足：{0:?}")]
    Validation(PlayerCommandIssue),
    #[error("账户对证券 {stock:?} 的总持仓为零")]
    NoHolding { stock: StockId },
    #[error("发行人证券代码无效：{detail}")]
    InvalidSecurityCode { detail: String },
    #[error("{0}")]
    Calendar(#[from] CalendarError),
    #[error("{0}")]
    Company(#[from] CompanySystemError),
    #[error("{0}")]
    Accounting(#[from] AccountingError),
    #[error("{0}")]
    Plan(#[from] CashDividendError),
    #[error("{0}")]
    Approval(#[from] CompanyCommandError),
    #[error("{0}")]
    StockApproval(#[from] StockDistributionApprovalError),
    #[error("{0}")]
    RightsPlan(#[from] RightsOfferingError),
    #[error("{0}")]
    SplitPlan(#[from] crate::company::share_split::ShareSplitError),
    #[error("{0}")]
    RepurchasePlan(#[from] crate::company::issuer_repurchase::IssuerRepurchaseError),
}

#[derive(Debug, thiserror::Error)]
#[error("玩家公司候选失败：账户 {account:?}，公司 {company}，类别 {kind:?}，日期 {day}：{cause}")]
pub(crate) struct PlayerCommandError {
    pub account: AccountId,
    pub company: CompanyId,
    pub kind: PlayerProposalKind,
    pub day: CivilDate,
    #[source]
    pub cause: PlayerCommandCause,
}

pub(super) fn player_holding_context(
    input: &PlayerCompanyCommandInput<'_>,
) -> Result<(CompanyId, StockCode, CalendarExchange), PlayerCommandError> {
    let company = input.proposal.company();
    let kind = input.proposal.kind();
    let fail = |cause| PlayerCommandError {
        account: input.requester.id(),
        company,
        kind,
        day: input.day,
        cause,
    };
    let invalid = |issue| fail(PlayerCommandCause::Validation(issue));
    let issuer = input
        .system
        .issuers()
        .get(&company)
        .ok_or_else(|| invalid(PlayerCommandIssue::UnknownIssuer))?;
    let code = issuer
        .listed_stock
        .as_ref()
        .ok_or_else(|| invalid(PlayerCommandIssue::NotListed))?;
    let stock = input
        .catalog
        .id(&code.0)
        .ok_or_else(|| invalid(PlayerCommandIssue::UnknownStock))?;
    let expected_exchange = StockExchange::from_a_share_code(code)
        .map_err(|detail| fail(PlayerCommandCause::InvalidSecurityCode { detail }))?;
    if input.security.code != *code || expected_exchange != input.security.exchange {
        return Err(invalid(PlayerCommandIssue::StockMismatch));
    }
    if !input
        .requester
        .position(&stock)
        .is_some_and(|position| position.qty() > 0)
    {
        return Err(fail(PlayerCommandCause::NoHolding { stock }));
    }
    let exchange = match input.security.exchange {
        StockExchange::Shanghai => CalendarExchange::Sse,
        StockExchange::Shenzhen => CalendarExchange::Szse,
    };
    Ok((company, code.clone(), exchange))
}

pub(crate) fn prepare_player_cash_dividend(
    input: PlayerCompanyCommandInput<'_>,
) -> Result<PreparedCashDividendApproval, PlayerCommandError> {
    let company = input.proposal.company();
    let kind = input.proposal.kind();
    let fail = |cause| PlayerCommandError {
        account: input.requester.id(),
        company,
        kind,
        day: input.day,
        cause,
    };
    let invalid = |issue| fail(PlayerCommandCause::Validation(issue));
    let PlayerCompanyProposal::CashDividend {
        gross_per_share, ..
    } = &input.proposal
    else {
        return Err(invalid(PlayerCommandIssue::UnexpectedProposalKind));
    };
    let (_, code, exchange) = player_holding_context(&input)?;
    if *gross_per_share <= Money::ZERO {
        return Err(invalid(PlayerCommandIssue::NonPositiveGross));
    }
    let (registered_on, ex_on) = crate::company::simple::preferences::preference_ex_dates(
        input.calendar,
        exchange,
        input.day,
    )
    .map_err(|cause| fail(cause.into()))?;
    let available = input
        .system
        .distributable_profit(&company)
        .map_err(|cause| fail(cause.into()))?
        .available_for_distribution;
    // Plan 的授权域是 Money，真实利润是 i128。只限制可表示授权额，不能因
    // 未使用余额超 i64 而拒绝小额请求；共同 declare 仍检查完整真实利润。
    let authorized = if available.cents() > i128::from(i64::MAX) {
        Money::from_cents(i64::MAX)
    } else {
        available.to_money().map_err(|cause| fail(cause.into()))?
    };
    // 公司方案身份沿只增账簿计数；不是交易 PlanId，不使用账户 allocator。
    let identity = format!(
        "player-proposal:{}:dividend:{}:{}",
        company.0,
        input.day,
        input.actions.dividends.len()
    );
    let plan = CashDividendPlan::new(
        identity,
        company,
        code.clone(),
        exchange,
        CashDividendFormula::StandardCashOnly,
        input.day,
        input.day,
        registered_on,
        ex_on,
        ex_on,
        *gross_per_share,
        authorized,
        input.calendar,
    )
    .map_err(|cause| fail(cause.into()))?;
    prepare_cash_dividend_approval(CashDividendApprovalInput {
        system: input.system,
        actions: input.actions,
        calendar: input.calendar,
        day: input.day,
        exchange,
        plan,
    })
    .map_err(|cause| fail(cause.into()))
}

pub(crate) fn prepare_player_stock_distribution(
    input: PlayerCompanyCommandInput<'_>,
    capital: &super::capital_approval_context::CapitalApprovalContext,
) -> Result<PreparedStockDistributionApproval, PlayerCommandError> {
    let company = input.proposal.company();
    let kind = input.proposal.kind();
    let fail = |cause| PlayerCommandError {
        account: input.requester.id(),
        company,
        kind,
        day: input.day,
        cause,
    };
    let invalid = |issue| fail(PlayerCommandCause::Validation(issue));
    let PlayerCompanyProposal::StockDistribution {
        kind: distribution_kind,
        shares_per_existing_share_micros: ratio,
        ..
    } = &input.proposal
    else {
        return Err(invalid(PlayerCommandIssue::UnexpectedProposalKind));
    };
    let (_, stock, exchange) = player_holding_context(&input)?;
    if *ratio == 0 {
        return Err(invalid(PlayerCommandIssue::NonPositiveRatio));
    }
    let registry = input
        .actions
        .registries
        .iter()
        .find(|registry| registry.issuer() == &company && registry.stock() == &stock)
        .ok_or_else(|| invalid(PlayerCommandIssue::MissingRegistry))?;
    let eligible = registry
        .holdings()
        .iter()
        .filter(|holding| {
            holding.holder != crate::company::share_registry::HolderId::IssuerTreasury
        })
        .flat_map(|holding| &holding.lots)
        .try_fold(0_u64, |total, lot| total.checked_add(lot.qty))
        .ok_or_else(|| invalid(PlayerCommandIssue::ShareCountOverflow))?;
    let total = u128::from(eligible)
        .checked_mul(u128::from(*ratio))
        .map(|product| product / 1_000_000)
        .and_then(|total| u64::try_from(total).ok())
        .ok_or_else(|| invalid(PlayerCommandIssue::ShareCountOverflow))?;
    if total == 0 {
        return Err(invalid(PlayerCommandIssue::NoNewShares));
    }
    let (registered_on, ex_rights_on) = crate::company::simple::preferences::preference_ex_dates(
        input.calendar,
        exchange,
        input.day,
    )
    .map_err(|cause| fail(cause.into()))?;
    let plan = crate::company::stock_distribution::StockDistributionEventPlan {
        event_id: format!(
            "player-proposal:{}:stock-distribution:{}:{}",
            company.0,
            input.day,
            input.actions.stock_distributions.len()
        ),
        approval_reference: format!("player-proposal:{:?}", input.requester.id()),
        issuer: company,
        stock,
        exchange,
        kind: distribution_kind.clone(),
        approved_on: input.day,
        announced_on: input.day,
        registered_on,
        ex_rights_on,
        shares_per_existing_share_micros: *ratio,
        approved_total_new_shares: total,
    };
    prepare_stock_distribution_approval(StockDistributionApprovalInput {
        system: input.system,
        actions: input.actions,
        calendar: input.calendar,
        day: input.day,
        exchange,
        plan,
        capital,
    })
    .map_err(|cause| fail(cause.into()))
}

/// 复用已审玩家 Plan 合同；这里只有输入构造，不表示共同批准或真实缴款。
pub(crate) fn build_player_rights_plan(
    input: PlayerCompanyCommandInput<'_>,
    rights_offering_enabled: bool,
) -> Result<RightsOfferingEventPlan, PlayerCommandError> {
    let company = input.proposal.company();
    let kind = input.proposal.kind();
    let fail = |cause| PlayerCommandError {
        account: input.requester.id(),
        company,
        kind,
        day: input.day,
        cause,
    };
    let invalid = |issue| fail(PlayerCommandCause::Validation(issue));
    let (price_per_share, mode, payment_days) = match &input.proposal {
        PlayerCompanyProposal::RightsOffering {
            price_per_share,
            shares_per_existing_share_micros,
            payment_days,
            ..
        } => (
            *price_per_share,
            RightsOfferingMode::RightsToAllShareholders {
                shares_per_existing_share_micros: *shares_per_existing_share_micros,
            },
            *payment_days,
        ),
        PlayerCompanyProposal::SecondaryOffering {
            price_per_share,
            shares,
            ..
        } => (
            *price_per_share,
            RightsOfferingMode::DirectedPlacement {
                targets: vec![DirectedPlacementTarget::NamedHolder {
                    holder: crate::company::share_registry::HolderId::Account(input.requester.id()),
                    shares: *shares,
                    lock_until: None,
                }],
            },
            // 已决玩家最小合同；无长期锁定是游戏简化，不能冒真实发行条件。
            1,
        ),
        _ => return Err(invalid(PlayerCommandIssue::UnexpectedProposalKind)),
    };
    let (_, stock, exchange) = player_holding_context(&input)?;
    if !rights_offering_enabled {
        return Err(invalid(PlayerCommandIssue::RightsOfferingDisabled));
    }
    let (registered_on, _) = crate::company::simple::preferences::preference_ex_dates(
        input.calendar,
        exchange,
        input.day,
    )
    .map_err(|cause| fail(cause.into()))?;
    let mut plan = RightsOfferingEventPlan {
        event_id: format!(
            "player-proposal:{}:{}:{}:{}",
            company.0,
            if kind == PlayerProposalKind::SecondaryOffering {
                "offering"
            } else {
                "rights"
            },
            input.day,
            input.actions.rights_offerings.len()
        ),
        approval_reference: format!("player-proposal:{:?}", input.requester.id()),
        issuer: company,
        stock,
        exchange,
        approved_on: input.day,
        announced_on: input.day,
        registered_on,
        // 真实 derive_schedule 在返回前覆盖完整日期，不发布半构造 Plan。
        payment_start_on: registered_on,
        payment_deadline_on: registered_on,
        ex_rights_on: registered_on,
        settlement_on: registered_on,
        listing_on: registered_on,
        price_per_share,
        mode,
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    };
    plan.derive_schedule(input.calendar, exchange, payment_days)
        .map_err(|cause| fail(cause.into()))?;
    plan.validate().map_err(|cause| fail(cause.into()))?;
    plan.validate_calendar(input.calendar)
        .map_err(|cause| fail(cause.into()))?;
    Ok(plan)
}

pub(super) fn build_player_split_plan(
    input: PlayerCompanyCommandInput<'_>,
) -> Result<crate::company::share_split::ShareSplitEventPlan, PlayerCommandError> {
    let company = input.proposal.company();
    let kind = input.proposal.kind();
    let fail = |cause| PlayerCommandError {
        account: input.requester.id(),
        company,
        kind,
        day: input.day,
        cause,
    };
    let PlayerCompanyProposal::ShareSplit {
        direction, ratio, ..
    } = &input.proposal
    else {
        return Err(fail(PlayerCommandCause::Validation(
            PlayerCommandIssue::UnexpectedProposalKind,
        )));
    };
    let (_, stock, exchange) = player_holding_context(&input)?;
    let (registered_on, ex_rights_on) = crate::company::simple::preferences::preference_ex_dates(
        input.calendar,
        exchange,
        input.day,
    )
    .map_err(|cause| fail(cause.into()))?;
    let plan = crate::company::share_split::ShareSplitEventPlan {
        event_id: format!(
            "player-proposal:{}:split:{}:{}",
            company.0,
            input.day,
            input.actions.share_splits.len()
        ),
        approval_reference: format!("player-proposal:{:?}", input.requester.id()),
        issuer: company,
        stock,
        exchange,
        direction: direction.clone(),
        ratio: *ratio,
        approved_on: input.day,
        announced_on: input.day,
        registered_on,
        ex_rights_on,
    };
    plan.validate().map_err(|cause| fail(cause.into()))?;
    plan.validate_calendar(input.calendar)
        .map_err(|cause| fail(cause.into()))?;
    Ok(plan)
}

/// 只构造真实当前证券单位下的方案；资金、计划身份和批准只由共同入口提交。
pub(super) fn build_player_repurchase_plan(
    input: PlayerCompanyCommandInput<'_>,
) -> Result<crate::company::issuer_repurchase::IssuerRepurchasePlan, PlayerCommandError> {
    use crate::company::issuer_repurchase::{IssuerRepurchasePlan, RepurchaseCompletionPolicy};
    let company = input.proposal.company();
    let kind = input.proposal.kind();
    let fail = |cause| PlayerCommandError {
        account: input.requester.id(),
        company,
        kind,
        day: input.day,
        cause,
    };
    let PlayerCompanyProposal::IssuerRepurchase {
        price_cap_per_share,
        total_budget,
        max_shares,
        window_trading_days,
        purpose,
        ..
    } = &input.proposal
    else {
        return Err(fail(PlayerCommandCause::Validation(
            PlayerCommandIssue::UnexpectedProposalKind,
        )));
    };
    let (_, stock, exchange) = player_holding_context(&input)?;
    if *window_trading_days == 0 {
        return Err(fail(PlayerCommandCause::Validation(
            PlayerCommandIssue::NonPositiveRepurchaseWindow,
        )));
    }
    // 公告后首个该交易所交易日起计，含起点共 N 个交易日；不按自然日相加。
    let window_start_on = input
        .calendar
        .next_trading_day(exchange, input.day)
        .map_err(|cause| fail(cause.into()))?;
    let mut window_deadline_on = window_start_on;
    for _ in 1..*window_trading_days {
        window_deadline_on = input
            .calendar
            .next_trading_day(exchange, window_deadline_on)
            .map_err(|cause| fail(cause.into()))?;
    }
    let approval_unit_event_id = input
        .actions
        .share_splits
        .iter()
        .filter(|book| {
            book.plan().stock == stock
                && book.receipt().is_some()
                && book.plan().ex_rights_on <= input.day
                && book.settled_on().is_some_and(|settled_on| settled_on <= input.day)
        })
        .max_by_key(|book| book.plan().ex_rights_on)
        .map(|book| book.plan().event_id.clone());
    let plan = IssuerRepurchasePlan {
        event_id: format!(
            "player-proposal:{}:repurchase:{}:{}",
            company.0,
            input.day,
            input.actions.issuer_repurchases.len()
        ),
        approval_reference: format!("player-proposal:{:?}", input.requester.id()),
        issuer: company,
        stock,
        exchange,
        approved_on: input.day,
        announced_on: input.day,
        window_start_on,
        window_deadline_on,
        price_cap_per_share: *price_cap_per_share,
        total_budget: *total_budget,
        max_shares: *max_shares,
        purpose: purpose.clone(),
        completion_policy: RepurchaseCompletionPolicy::CancelOnCompletion,
        approval_unit_event_id,
    };
    plan.validate().map_err(|cause| fail(cause.into()))?;
    plan.validate_calendar(input.calendar)
        .map_err(|cause| fail(cause.into()))?;
    Ok(plan)
}
