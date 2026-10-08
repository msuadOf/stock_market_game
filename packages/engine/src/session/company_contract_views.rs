//! F 批共同契约 session 视图构建器（全部为只读投影）。
//!
//! 三块内容：
//! 1. [`owner_rights_views`]：账户维度的配股权证/额度/缴款窗口视图（复用
//!    `rights_offerings` books 与 `rights_subscription_queue` 既有事实）；
//! 2. [`active_plan_views`]：五类行为账簿中未完成方案的阶段+关键日期投影；
//! 3. [`company_capabilities_view`]：完整能力面（公司事实 + 方案 + 各行为
//!    业务条件 + 本人权利摘要），owner 隔离（只含查询账户本人的事实）。
//!
//! 「业务条件满足」只表示按当前事实可评估的前置条件，不承诺受理；正式
//! 受理仍执行完整制度校验（A 股语义门禁）。

use super::corporate_actions::{
    OwnerEntitlementDetail, OwnerQueuedSubscriptionView, OwnerRightsOfferingView,
    OwnerSettledSubscriptionView, QueuedRightsSubscription, SessionCorporateActions,
    SessionCorporateActionsError,
};
use crate::calendar::CivilDate;
use crate::company::capabilities::{
    ActiveCorporatePlanView, ActivePlanKeyDate, ActivePlanStage, CompanyCapabilities,
    CorporateActionKind, CorporateActionReadiness, OwnerRightsSummaryView, PaymentWindowState,
};
use crate::company::cash_dividend::CashDividendStatus;
use crate::company::issuer_repurchase::IssuerRepurchaseStatus;
use crate::company::rights_offering::RightsOfferingStatus;
use crate::company::share_registry::HolderId;
use crate::company::share_split::ShareSplitStatus;
use crate::company::stock_distribution::StockDistributionStatus;
use crate::company::CompanyId;
use crate::company::CompanySystem;
use crate::orderbook::AccountId;

fn key_date(label: &str, date: CivilDate) -> ActivePlanKeyDate {
    ActivePlanKeyDate {
        label: label.to_string(),
        date: date.to_iso(),
    }
}

fn rights_stage(status: &RightsOfferingStatus) -> ActivePlanStage {
    match status {
        RightsOfferingStatus::Approved => ActivePlanStage::Approved,
        RightsOfferingStatus::Announced => ActivePlanStage::Announced,
        RightsOfferingStatus::Entitled => ActivePlanStage::Entitled,
        RightsOfferingStatus::Closed => ActivePlanStage::Closed,
        // Settled 为终态：调用方已过滤，不会到达。
        RightsOfferingStatus::Settled => ActivePlanStage::Closed,
    }
}

/// 账户维度的未完成配股方案视图（owner 隔离；Settled 终态不再出现）。
pub(crate) fn owner_rights_views(
    actions: &SessionCorporateActions,
    account: AccountId,
    today: CivilDate,
) -> Result<Vec<OwnerRightsOfferingView>, SessionCorporateActionsError> {
    let holder = HolderId::Account(account);
    let mut views = Vec::new();
    for book in &actions.rights_offerings {
        if matches!(book.status(), RightsOfferingStatus::Settled) {
            continue;
        }
        let plan = book.plan();
        let payment_window = if today < plan.payment_start_on {
            PaymentWindowState::BeforeOpen
        } else if today <= plan.payment_deadline_on {
            PaymentWindowState::Open
        } else {
            PaymentWindowState::Closed
        };
        let owner_entitlement = book
            .entitlement()
            .and_then(|receipt| {
                receipt
                    .entitlements
                    .iter()
                    .find(|entry| entry.holder == holder)
            })
            .map(|entry| OwnerEntitlementDetail {
                rights_shares: entry.rights_shares,
                lock_until: entry.lock_until,
            });
        let open_subscription_remaining_shares = match book.entitlement() {
            // 权证未派发（R 日未到）时公开额度尚未冻结，不冒充可用。
            None => None,
            Some(_) if owner_entitlement.is_some() => None,
            Some(receipt) => Some(
                open_public_remaining(receipt.open_subscription_shares, book, &actions
                    .rights_subscription_queue)?
                .to_string(),
            ),
        };
        let queued_subscription = actions
            .rights_subscription_queue
            .iter()
            .find(|queued| queued.event_id == plan.event_id && queued.account == account)
            .map(|QueuedRightsSubscription {
                     event_id: _,
                     account: _,
                     requested_shares,
                     submitted_on,
                 }| OwnerQueuedSubscriptionView {
                requested_shares: *requested_shares,
                submitted_on: *submitted_on,
            });
        let settled_subscription = book
            .subscriptions()
            .iter()
            .find(|record| record.holder == holder)
            .map(|record| OwnerSettledSubscriptionView {
                requested_shares: record.requested_shares,
                paid_shares: record.paid_shares,
                paid_amount: record.paid_amount,
                waived_shares: record.waived_shares,
            });
        views.push(OwnerRightsOfferingView {
            event_id: plan.event_id.clone(),
            stock: plan.stock.clone(),
            issuer: plan.issuer.0.clone(),
            stage: rights_stage(book.status()),
            payment_window,
            price_per_share: plan.price_per_share,
            payment_start_on: plan.payment_start_on,
            payment_deadline_on: plan.payment_deadline_on,
            ex_rights_on: plan.ex_rights_on,
            settlement_on: plan.settlement_on,
            owner_entitlement,
            open_subscription_remaining_shares,
            queued_subscription,
            settled_subscription,
        });
    }
    Ok(views)
}

/// 公开配售剩余额度：`额度 − 已入账非具名认购 − 排队非具名认购`。
/// 与 [`crate::session::GameSession::subscribe_rights_offering`] 的受理口径
/// 同构；溢出或透支属于状态不一致，显式报错不静默钳制。
fn open_public_remaining(
    open_subscription_shares: u64,
    book: &crate::company::rights_offering::RightsOfferingBook,
    queue: &[QueuedRightsSubscription],
) -> Result<u64, SessionCorporateActionsError> {
    let has_named_entitlement = |holder: &HolderId| {
        book.entitlement()
            .is_some_and(|receipt| receipt.entitlements.iter().any(|entry| &entry.holder == holder))
    };
    let mut settled_open_used = 0_u64;
    for record in book.subscriptions() {
        if !has_named_entitlement(&record.holder) {
            settled_open_used = settled_open_used
                .checked_add(record.requested_shares)
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("公开配售已入账认购合计溢出".into())
                })?;
        }
    }
    let mut queued_open_used = 0_u64;
    for queued in queue {
        if queued.event_id == book.plan().event_id
            && !has_named_entitlement(&HolderId::Account(queued.account))
        {
            queued_open_used = queued_open_used
                .checked_add(queued.requested_shares)
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("公开配售排队认购合计溢出".into())
                })?;
        }
    }
    open_subscription_shares
        .checked_sub(settled_open_used)
        .and_then(|value| value.checked_sub(queued_open_used))
        .ok_or_else(|| {
            SessionCorporateActionsError::Invalid("公开配售额度与已受理认购不一致".into())
        })
}

/// 五类行为账簿中该公司未完成方案的阶段+关键日期投影。
pub(crate) fn active_plan_views(
    actions: &SessionCorporateActions,
    company: &CompanyId,
) -> Vec<ActiveCorporatePlanView> {
    let mut plans = Vec::new();
    for book in &actions.dividends {
        let plan = book.plan();
        if plan.issuer != *company || matches!(book.status(), CashDividendStatus::Paid) {
            continue;
        }
        let stage = match book.status() {
            CashDividendStatus::Approved => ActivePlanStage::Approved,
            CashDividendStatus::Announced => ActivePlanStage::Announced,
            CashDividendStatus::Registered => ActivePlanStage::Registered,
            CashDividendStatus::Payable => ActivePlanStage::Payable,
            CashDividendStatus::PartiallyPaid => ActivePlanStage::PartiallyPaid,
            CashDividendStatus::Paid => unreachable!("Paid 已在上方过滤"),
        };
        plans.push(ActiveCorporatePlanView {
            kind: CorporateActionKind::CashDividend,
            identity: plan.plan_id.clone(),
            stage,
            key_dates: vec![
                key_date("批准日", plan.approved_on),
                key_date("公告日", plan.announced_on),
                key_date("股权登记日", plan.registered_on),
                key_date("除息日", plan.ex_dividend_on),
                key_date("派息日", plan.payable_on),
            ],
        });
    }
    for book in &actions.stock_distributions {
        let plan = book.plan();
        if plan.issuer != *company || matches!(book.status(), StockDistributionStatus::Credited) {
            continue;
        }
        let stage = match book.status() {
            StockDistributionStatus::Approved => ActivePlanStage::Approved,
            StockDistributionStatus::Announced => ActivePlanStage::Announced,
            StockDistributionStatus::Registered => ActivePlanStage::Registered,
            StockDistributionStatus::Credited => unreachable!("Credited 已在上方过滤"),
        };
        plans.push(ActiveCorporatePlanView {
            kind: CorporateActionKind::StockDistribution,
            identity: plan.event_id.clone(),
            stage,
            key_dates: vec![
                key_date("批准日", plan.approved_on),
                key_date("公告日", plan.announced_on),
                key_date("股权登记日", plan.registered_on),
                key_date("除权日", plan.ex_rights_on),
            ],
        });
    }
    for book in &actions.share_splits {
        let plan = book.plan();
        if plan.issuer != *company || matches!(book.status(), ShareSplitStatus::Settled) {
            continue;
        }
        let stage = match book.status() {
            ShareSplitStatus::Approved => ActivePlanStage::Approved,
            ShareSplitStatus::Announced => ActivePlanStage::Announced,
            ShareSplitStatus::Registered => ActivePlanStage::Registered,
            ShareSplitStatus::Settled => unreachable!("Settled 已在上方过滤"),
        };
        plans.push(ActiveCorporatePlanView {
            kind: CorporateActionKind::ShareSplit,
            identity: plan.event_id.clone(),
            stage,
            key_dates: vec![
                key_date("批准日", plan.approved_on),
                key_date("公告日", plan.announced_on),
                key_date("股权登记日", plan.registered_on),
                key_date("除权日", plan.ex_rights_on),
            ],
        });
    }
    for book in &actions.rights_offerings {
        let plan = book.plan();
        if plan.issuer != *company || matches!(book.status(), RightsOfferingStatus::Settled) {
            continue;
        }
        plans.push(ActiveCorporatePlanView {
            kind: CorporateActionKind::RightsOffering,
            identity: plan.event_id.clone(),
            stage: rights_stage(book.status()),
            key_dates: vec![
                key_date("批准日", plan.approved_on),
                key_date("公告日", plan.announced_on),
                key_date("股权登记日", plan.registered_on),
                key_date("缴款起始日", plan.payment_start_on),
                key_date("缴款截止日", plan.payment_deadline_on),
                key_date("除权日", plan.ex_rights_on),
                key_date("划款日", plan.settlement_on),
            ],
        });
    }
    for book in &actions.issuer_repurchases {
        let plan = book.plan();
        if plan.issuer != *company || matches!(book.status(), IssuerRepurchaseStatus::Cancelled) {
            continue;
        }
        let stage = match book.status() {
            IssuerRepurchaseStatus::Approved => ActivePlanStage::Approved,
            IssuerRepurchaseStatus::Announced => ActivePlanStage::Announced,
            IssuerRepurchaseStatus::Executing => ActivePlanStage::Executing,
            // Completed 为 H 批默认注销策略下的短暂过渡态（完成判定后的首个
            // 日终自动注销；零成交方案无股可核减、保持本终态）。
            IssuerRepurchaseStatus::Completed => ActivePlanStage::Completed,
            IssuerRepurchaseStatus::Cancelled => unreachable!("Cancelled 已在上方过滤"),
        };
        plans.push(ActiveCorporatePlanView {
            kind: CorporateActionKind::IssuerRepurchase,
            identity: plan.event_id.clone(),
            stage,
            key_dates: vec![
                key_date("批准日", plan.approved_on),
                key_date("公告日", plan.announced_on),
                key_date("执行窗口起始日", plan.window_start_on),
                key_date("执行窗口截止日", plan.window_deadline_on),
            ],
        });
    }
    plans.sort_by(|left, right| {
        // 确定性排序：按类别固定序（与 action_readiness 同序）+ 方案身份。
        let rank = |kind: CorporateActionKind| match kind {
            CorporateActionKind::CashDividend => 0,
            CorporateActionKind::StockDistribution => 1,
            CorporateActionKind::RightsOffering => 2,
            CorporateActionKind::IssuerRepurchase => 3,
            CorporateActionKind::ShareSplit => 4,
        };
        rank(left.kind)
            .cmp(&rank(right.kind))
            .then_with(|| left.identity.cmp(&right.identity))
    });
    plans
}

/// 各行为业务条件快照（固定五类顺序；blockers 为空 = 可评估前置条件满足）。
/// `distributable` 为调用方已取得的同一查询结果（与能力面快照共享，一次装配
/// 只查一次；F 修复轮 low-6）。
fn action_readiness(
    company: &CompanyId,
    company_system: &CompanySystem,
    actions: &SessionCorporateActions,
    rights_enabled: bool,
    repurchase_enabled: bool,
    distributable: &Result<
        crate::company::DistributableProfit,
        crate::company::CompanySystemError,
    >,
) -> Vec<CorporateActionReadiness> {
    let legal_facts_bound = company_system
        .dividend_legal_facts(company)
        .map(|facts| facts.is_some())
        .unwrap_or(false);
    let distributable_positive = distributable
        .as_ref()
        .map(|profit| profit.available_for_distribution.is_positive())
        .unwrap_or(false);

    // 现金分红：法定事实 + 可分配利润快照可用且为正。
    let mut cash_blockers = Vec::new();
    if !legal_facts_bound {
        cash_blockers.push("未绑定注册资本法定事实（define_dividend_legal_facts）".into());
    }
    match distributable {
        Ok(profit) if profit.available_for_distribution.is_positive() => {}
        Ok(_) => cash_blockers.push("可分配利润为零或负，无可分配现金".into()),
        Err(error) => cash_blockers.push(format!("可分配利润快照不可用：{error}")),
    }

    // 送转：法定事实 +（送股来源：可分配利润 > 0，或转增来源：资本公积贷方余额 > 0）。
    let mut stock_blockers = Vec::new();
    if !legal_facts_bound {
        stock_blockers.push("送转面值推导需要先显式绑定公司注册资本法定事实".into());
    }
    let conversion_reserve_positive = company_system
        .finance(company)
        .ok()
        .and_then(|finance| {
            finance
                .books()
                .ledger()
                .account_net_debit(&crate::accounting::LedgerAccountId(
                    crate::accounting::reports::simple_summary::CAPITAL_RESERVE.into(),
                ))
                .ok()
        })
        .and_then(|net| net.neg().ok())
        .is_some_and(|balance| balance.is_positive());
    if !distributable_positive && !conversion_reserve_positive {
        stock_blockers.push(
            "送股来源（可分配利润）与转增来源（资本公积贷方余额）均不可用".into(),
        );
    }

    // 配股：新局开关 + 法定事实（受理时按注册资本推导每股面值）。
    let mut rights_blockers = Vec::new();
    if !rights_enabled {
        rights_blockers.push("新局未启用配股／增发机制（rights_offering_enabled=false）".into());
    }
    if !legal_facts_bound && company_system.current_par_value(company).ok().flatten().is_none() {
        rights_blockers.push("未绑定注册资本法定事实且无既有面值锚，无法推导每股面值".into());
    }

    // 回购：新局开关 + 同证券唯一未完成方案（受理预检）。唯一性口径与
    // `approve_issuer_repurchase` 一致：只挡非终态（Approved/Announced/Executing）
    // 方案；Completed 不阻塞新方案受理（F 修复轮建议-3：曾把 Completed 也计入
    // 阻塞，与受理口径反向漂移——报「不满足」但实际可受理）。H 批起完成方案
    // 于完成判定后的首个日终自动注销（默认注销策略），Completed 只是过渡态；
    // 零成交方案无股份可核减、保持 Completed 终态，同样不阻塞。
    let mut repurchase_blockers = Vec::new();
    if !repurchase_enabled {
        repurchase_blockers.push("新局未启用发行人回购机制（issuer_repurchase_enabled=false）".into());
    } else if actions.issuer_repurchases.iter().any(|book| {
        book.plan().issuer == *company
            && !matches!(
                book.status(),
                IssuerRepurchaseStatus::Completed | IssuerRepurchaseStatus::Cancelled
            )
    }) {
        repurchase_blockers.push(
            "同证券已存在未完成回购方案（唯一性预检；既有方案完成后的首个日终自动注销，之后可提新方案）".into(),
        );
    }

    // 拆股／缩股：受理的静态前置——法定事实为硬前置（`approve_share_split`
    // 无条件要求 `registered_capital_at_approval`），无既有面值锚时须能由
    // 注册资本 ÷ 发行股数整除推导。日期有序性、同除权日碰撞三层预检与比例
    // 校验依赖具体方案输入，静态快照无从评估，不在此冒充可判定（F 修复轮
    // low-4：曾恒 ready，与受理依赖不一致）。
    let mut split_blockers = Vec::new();
    if !legal_facts_bound {
        split_blockers.push(
            "拆股／缩股受理需要先显式绑定公司注册资本法定事实（registered_capital_at_approval）"
                .into(),
        );
    } else if company_system.current_par_value(company).ok().flatten().is_none() {
        // 无既有面值锚：面值须由法定事实整除推导（对齐 rights blocker 口径）。
        let capital_divisible = company_system
            .dividend_legal_facts(company)
            .ok()
            .flatten()
            .zip(company_system.issuers().get(company))
            .and_then(|(facts, spec)| {
                let capital = i128::from(facts.registered_capital.to_money().ok()?.cents());
                let shares = i128::from(spec.issued_shares);
                (shares > 0).then_some(capital % shares == 0)
            })
            .unwrap_or(false);
        if !capital_divisible {
            split_blockers.push(
                "注册资本与发行股数不能整除为每股面值；需先提供可整除的法定事实或既有面值锚"
                    .into(),
            );
        }
    }

    let build = |kind: CorporateActionKind, blockers: Vec<String>| CorporateActionReadiness {
        kind,
        ready: blockers.is_empty(),
        blockers,
    };
    vec![
        build(CorporateActionKind::CashDividend, cash_blockers),
        build(CorporateActionKind::StockDistribution, stock_blockers),
        build(CorporateActionKind::RightsOffering, rights_blockers),
        build(CorporateActionKind::IssuerRepurchase, repurchase_blockers),
        build(CorporateActionKind::ShareSplit, split_blockers),
    ]
}

/// 完整能力面（公司事实 + 未完成方案 + 各行为条件 + 本人权利摘要）。
pub(crate) fn company_capabilities_view(
    company: &CompanyId,
    account: AccountId,
    company_system: &CompanySystem,
    actions: &SessionCorporateActions,
    rights_enabled: bool,
    repurchase_enabled: bool,
    today: CivilDate,
) -> Result<CompanyCapabilities, SessionCorporateActionsError> {
    // 可分配利润一次装配只查一次：facts 快照与 readiness 判定共享同一结果
    // （F 修复轮 low-6：此前 company_facts 一次、readiness 两次，共三次）。
    let distributable = company_system.distributable_profit(company);
    let action_readiness_snapshot = action_readiness(
        company,
        company_system,
        actions,
        rights_enabled,
        repurchase_enabled,
        &distributable,
    );
    let facts = company_system
        .company_facts_with_distributable(company, distributable)
        .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
    let owner_views = owner_rights_views(actions, account, today)?;
    let owner_rights = owner_views
        .iter()
        .map(|view| OwnerRightsSummaryView {
            event_id: view.event_id.clone(),
            stock: view.stock.0.clone(),
            stage: view.stage,
            payment_window: view.payment_window,
            entitled_shares: view
                .owner_entitlement
                .as_ref()
                .map(|entitlement| entitlement.rights_shares.to_string()),
            open_subscription_remaining: view.open_subscription_remaining_shares.clone(),
        })
        .collect();
    Ok(CompanyCapabilities {
        revenue: facts.revenue,
        net_income: facts.net_income,
        equity: facts.equity,
        cash_flow: facts.cash_flow,
        full_financial_statements: facts.full_financial_statements,
        cash_settlement: facts.cash_settlement,
        unsupported_reason: facts.unsupported_reason,
        par_value_per_share: facts.par_value_per_share,
        issued_shares: facts.issued_shares,
        registered_capital: facts.registered_capital,
        distributable_profit: facts.distributable_profit,
        active_plans: active_plan_views(actions, company),
        action_readiness: action_readiness_snapshot,
        owner_rights,
    })
}
