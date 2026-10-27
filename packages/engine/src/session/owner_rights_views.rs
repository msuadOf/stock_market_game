//! 本人配股查询的公共只读类型；股数与 Money 沿用当前精确 wire 单位。

use crate::account::StockCode;

/// 本人在单个未完成配股方案中的具名权利明细（权证派发后存在）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerEntitlementDetail {
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub rights_shares: u64,
    /// 定向锁定期截止日（None = 无锁；配股面向全体股东模式恒为 None）。
    pub lock_until: Option<crate::calendar::CivilDate>,
}

/// 本人已提交、待当日日终划扣的排队认购。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerQueuedSubscriptionView {
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub requested_shares: u64,
    pub submitted_on: crate::calendar::CivilDate,
}

/// 本人已进入账簿的认购记录（缴款日终划扣后的事实）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerSettledSubscriptionView {
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub requested_shares: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub paid_shares: u64,
    pub paid_amount: crate::money::Money,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub waived_shares: u64,
}

/// 本人在单个未完成配股方案中的完整只读视图（F 批共同契约；复用
/// rights_offering 既有事实，含窗口状态、价格、日程与认购进度）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OwnerRightsOfferingView {
    pub event_id: String,
    pub stock: StockCode,
    pub issuer: crate::company::CompanyId,
    pub stage: crate::company::capabilities::ActivePlanStage,
    pub payment_window: crate::company::capabilities::PaymentWindowState,
    pub price_per_share: crate::money::Money,
    pub payment_start_on: crate::calendar::CivilDate,
    pub payment_deadline_on: crate::calendar::CivilDate,
    pub ex_rights_on: crate::calendar::CivilDate,
    pub settlement_on: crate::calendar::CivilDate,
    pub owner_entitlement: Option<OwnerEntitlementDetail>,
    /// 公开配售剩余额度（规范 u64 十进制字符串）：权证已派发且本人无具名
    /// 权利且方案有公开额度时给出；`Some("0")` 表示额度已用尽。
    pub open_subscription_remaining_shares: Option<String>,
    pub queued_subscription: Option<OwnerQueuedSubscriptionView>,
    pub settled_subscription: Option<OwnerSettledSubscriptionView>,
}
