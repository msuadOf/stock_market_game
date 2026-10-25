//! 公司能力与本人配股查询：只读现有事实，不受理认购或推进状态机。

use super::corporate_actions::{
    QueuedRightsSubscription, SessionCorporateActions, SessionCorporateActionsError,
};
use super::{
    GameSession, OwnerEntitlementDetail, OwnerQueuedSubscriptionView, OwnerRightsOfferingView,
    OwnerSettledSubscriptionView,
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
use crate::company::{CompanyId, CompanySystem};
use crate::orderbook::AccountId;

fn invalid(detail: impl Into<String>) -> SessionCorporateActionsError {
    SessionCorporateActionsError::Invalid(detail.into())
}

impl GameSession {
    fn capability_query_context(
        &self,
        account: AccountId,
    ) -> Result<(&CompanySystem, &SessionCorporateActions), SessionCorporateActionsError> {
        if !self.state.accounts.contains_key(&account) {
            return Err(invalid(format!("公司查询账户 {account:?} 不存在")));
        }
        match (self.company_system(), self.corporate_actions()) {
            (Some(system), Some(actions)) => Ok((system, actions)),
            (None, None) => Err(invalid("当前会话未配置公司系统，无公司权益事实可查")),
            _ => Err(invalid("公司系统与公司行为容器在场性不一致")),
        }
    }

    /// 公司当前事实、未完成方案与本人权利的只读快照。
    /// 当前命令和真实投资者结算尚未接通，五类行为均显式报告 blocker。
    pub fn company_capabilities(
        &self,
        company: &CompanyId,
        account: AccountId,
    ) -> Result<CompanyCapabilities, SessionCorporateActionsError> {
        let (system, actions) = self.capability_query_context(account)?;
        let facts = system
            .company_facts(company)
            .map_err(|error| invalid(error.to_string()))?;
        let owner_rights = owner_rights_views(actions, account, self.civil_date())?
            .into_iter()
            .filter(|view| view.issuer == *company)
            .map(|view| OwnerRightsSummaryView {
                event_id: view.event_id,
                stock: view.stock.0,
                stage: view.stage,
                payment_window: view.payment_window,
                entitled_shares: view
                    .owner_entitlement
                    .map(|entry| entry.rights_shares.to_string()),
                open_subscription_remaining: view.open_subscription_remaining_shares,
            })
            .collect();
        let action_readiness = [
            CorporateActionKind::CashDividend,
            CorporateActionKind::StockDistribution,
            CorporateActionKind::RightsOffering,
            CorporateActionKind::IssuerRepurchase,
            CorporateActionKind::ShareSplit,
        ]
        .into_iter()
        .map(|kind| {
            let mut blockers =
                vec!["公司行为命令与真实投资者结算尚未接线，当前仅支持只读查询".to_owned()];
            if kind == CorporateActionKind::RightsOffering
                && !self.state.setup.rights_offering_enabled
            {
                blockers.push("新局未启用配股／增发机制（rights_offering_enabled=false）".into());
            }
            if kind == CorporateActionKind::IssuerRepurchase
                && !self.state.setup.issuer_repurchase_enabled
            {
                blockers.push("新局未启用发行人回购机制（issuer_repurchase_enabled=false）".into());
            }
            CorporateActionReadiness {
                kind,
                ready: false,
                blockers,
            }
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
            action_readiness,
            owner_rights,
        })
    }

    /// 当前未完成配股方案的本人权利与缴款窗口；未知账户及无公司局显式报错。
    pub fn owner_rights_offerings(
        &self,
        account: AccountId,
    ) -> Result<Vec<OwnerRightsOfferingView>, SessionCorporateActionsError> {
        let (_, actions) = self.capability_query_context(account)?;
        owner_rights_views(actions, account, self.civil_date())
    }
}

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
        RightsOfferingStatus::Settled => unreachable!("已结算方案不进入当前视图"),
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
            Some(receipt)
                if owner_entitlement.is_some() || receipt.open_subscription_shares == 0 =>
            {
                None
            }
            Some(receipt) => Some(
                open_public_remaining(
                    receipt.open_subscription_shares,
                    book,
                    &actions.rights_subscription_queue,
                )?
                .to_string(),
            ),
        };
        let queued_subscription = actions
            .rights_subscription_queue
            .iter()
            .find(|queued| queued.event_id == plan.event_id && queued.account == account)
            .map(
                |QueuedRightsSubscription {
                     event_id: _,
                     account: _,
                     requested_shares,
                     submitted_on,
                 }| OwnerQueuedSubscriptionView {
                    requested_shares: *requested_shares,
                    submitted_on: *submitted_on,
                },
            );
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
            issuer: plan.issuer,
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
/// 账簿与排队记录按 requested_shares 计占用；溢出或透支显式报错。
fn open_public_remaining(
    open_subscription_shares: u64,
    book: &crate::company::rights_offering::RightsOfferingBook,
    queue: &[QueuedRightsSubscription],
) -> Result<u64, SessionCorporateActionsError> {
    let has_named_entitlement = |holder: &HolderId| {
        book.entitlement().is_some_and(|receipt| {
            receipt
                .entitlements
                .iter()
                .any(|entry| &entry.holder == holder)
        })
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
