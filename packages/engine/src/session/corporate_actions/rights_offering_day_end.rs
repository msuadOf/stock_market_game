//! 配股／增发的日终纯候选。所有 paid/refunded 数值都表示待提交事实，不能冒充组端执行回执。

use super::{QueuedRightsSubscription, RejectedRightsSubscription, SessionCorporateActions};
use crate::account::{AccountKind, StockCode};
use crate::calendar::{CivilDate, TradingCalendar};
use crate::company::cash_dividend_tax::CashDividendTaxBook;
use crate::company::rights_offering::{
    DirectedPlacementTarget, RightsOfferingBook, RightsOfferingError, RightsOfferingEventPlan,
    RightsOfferingMode, RightsSubscriptionRecord, SubscriptionOrigin,
};
use crate::company::share_registry::ShareRegistry;
use crate::company::CompanyId;
use crate::money::Money;
use crate::orderbook::AccountId;
use std::collections::BTreeMap;
use thiserror::Error;

/// 真实日终组账户资源；available_cash 已扣在途占用，不得使用镜像总现金代替。
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct RightsFinalResources {
    pub available_cash: Money,
    pub kind: AccountKind,
}

/// 现有队列的一账户一净申请身份；不得把受理成功解释成缴款完成。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RightsRequestKey {
    pub event_id: String,
    pub account: AccountId,
    pub submitted_on: CivilDate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RightsAccountDisposition {
    PreparedSubscription,
    PreparedSettlement,
    Rejected { cause: RightsOfferingError },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RightsAccountResult {
    pub request: RightsRequestKey,
    pub origin: SubscriptionOrigin,
    pub requested_shares: u64,
    pub paid_shares: u64,
    pub waived_shares: u64,
    pub paid_amount: Money,
    pub refunded_amount: Money,
    pub disposition: RightsAccountDisposition,
}

/// 独立的配股领域意图。D1a 既有股利／税款操作不能替代这些语义。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum RightsOfferingIntent {
    PaymentDebit { request: RightsRequestKey, amount: Money },
    FailureRefund { request: RightsRequestKey, amount: Money },
    /// paid_amount 保留实际认购成本来源；是否及如何累加 invested 由后续行政操作契约决定。
    NewShares { request: RightsRequestKey, stock: StockCode, shares: u64, paid_amount: Money },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RightsFinanceSettlement {
    pub issuer: CompanyId,
    pub event_id: String,
    pub settled_on: CivilDate,
    pub issued_shares: u64,
    pub proceeds: Money,
}

/// 仅本簇会变化的候选字段；调用方在全部组回执勾稽后才可安装。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RightsOfferingDayEndCandidate {
    pub registries: Vec<ShareRegistry>,
    pub dividend_tax_books: Vec<CashDividendTaxBook>,
    pub rights_offerings: Vec<RightsOfferingBook>,
    pub rights_subscription_queue: Vec<QueuedRightsSubscription>,
    pub rejected_rights_subscriptions: Vec<RejectedRightsSubscription>,
    pub results: Vec<RightsAccountResult>,
    pub intents: Vec<RightsOfferingIntent>,
    pub finance_settlements: Vec<RightsFinanceSettlement>,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub(crate) enum RightsOfferingCandidateError {
    #[error("配股事件 {event_id} 尚不支持长期限售，lock_until={lock_until}")]
    UnsupportedLock { event_id: String, lock_until: CivilDate },
    #[error("配股事件 {event_id} 在 {day} 缺少账户 {account:?} 的最终可用资源")]
    MissingAccount { event_id: String, account: AccountId, day: CivilDate },
    #[error("配股事件 {event_id} 在 {day} 的账户 {account:?} 可用现金为负：{cash:?}")]
    NegativeCash { event_id: String, account: AccountId, day: CivilDate, cash: Money },
    #[error("配股事件 {event_id} 在 {day} 的领域校验失败：{source}")]
    Domain { event_id: String, day: CivilDate, source: RightsOfferingError },
    #[error("配股事件 {event_id} 在 {day} 的候选状态错误：{detail}")]
    Invalid { event_id: String, day: CivilDate, detail: String },
}

/// Session 命令和恢复接线必须复用的能力门禁；不删除纯公司领域表达限售的能力。
pub(crate) fn validate_rights_offering_capability(
    plan: &RightsOfferingEventPlan,
) -> Result<(), RightsOfferingCandidateError> {
    if let RightsOfferingMode::DirectedPlacement { targets } = &plan.mode {
        for target in targets {
            if let DirectedPlacementTarget::NamedHolder { lock_until: Some(lock_until), .. } = target {
                return Err(RightsOfferingCandidateError::UnsupportedLock {
                    event_id: plan.event_id.clone(), lock_until: *lock_until,
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn rights_offering_day_end_candidate(
    actions: &SessionCorporateActions,
    day: CivilDate,
    calendar: &TradingCalendar,
    resources: &BTreeMap<AccountId, RightsFinalResources>,
) -> Result<RightsOfferingDayEndCandidate, RightsOfferingCandidateError> {
    Ok(RightsOfferingDayEndCandidate {
        registries: actions.registries.clone(),
        dividend_tax_books: actions.dividend_tax_books.clone(),
        rights_offerings: actions.rights_offerings.clone(),
        rights_subscription_queue: actions.rights_subscription_queue.clone(),
        rejected_rights_subscriptions: actions.rejected_rights_subscriptions.clone(),
        results: vec![], intents: vec![], finance_settlements: vec![],
    })
}

#[cfg(test)]
#[path = "rights_offering_day_end_tests.rs"]
mod tests;
