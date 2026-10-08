//! 公司共同契约能力面（R4 最小公共合同，F 批收口）。
//!
//! 原静态布尔位保留语义不变；新增「当前事实」结构化视图（每股面值、现行
//! 总股本、可分配利润快照、法定事实、当前未完成方案、各行为业务条件与
//! 本人按账户的权利/额度）。金额单位与公开报表一致：`AccountingAmount`
//! 来源用元字符串（同 [`crate::company::query::PublicReportLine`]），`Money`
//! 来源用十进制分字符串（ADR-0031）。不可用**显式给 reason，不填零**。
//! 本文件只定义公共类型；公司侧事实由
//! [`crate::company::CompanySystem::company_facts`] 填充，方案/条件/本人
//! 部分由 session 层组装成 [`CompanyCapabilities`]，不开放内部账簿。

/// 元字符串口径的金额事实（与公开报表同单位）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum CapabilityAmount {
    Available {
        /// 精确十进制元字符串（`AccountingAmount::to_yuan_string`），不用 JS number。
        amount_yuan: String,
    },
    Unavailable {
        reason: String,
    },
}

/// 分字符串口径的金额事实（ADR-0031 `Money` wire：十进制分字符串）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum CapabilityMoney {
    Available {
        /// 精确十进制分字符串（`Money::cents()`）。
        cents: String,
    },
    Unavailable {
        reason: String,
    },
}

/// 可分配利润快照（三者同源同可用性；不可用整体给 reason，不部分填零）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum DistributableProfitSnapshot {
    Available {
        /// 亏损弥补后累积（元字符串）。
        accumulated_after_loss_yuan: String,
        /// 法定公积金（元字符串）。
        statutory_reserve_yuan: String,
        /// 可供分配利润（元字符串）。
        available_for_distribution_yuan: String,
        /// 免计提基准年度（已达注册资本 50% 免计提时给出）。
        reserve_basis_year: Option<i32>,
    },
    Unavailable {
        reason: String,
    },
}

/// 共同契约五类公司行为。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum CorporateActionKind {
    CashDividend,
    StockDistribution,
    RightsOffering,
    IssuerRepurchase,
    ShareSplit,
}

/// 五类行为状态机阶段的公共超集（各模块私有状态机只读投影；不含各状态机
/// 已终止后的「终态」，本视图只承载未完成方案）。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum ActivePlanStage {
    Approved,
    Announced,
    Registered,
    Entitled,
    Closed,
    Payable,
    PartiallyPaid,
    Executing,
    Completed,
}

/// 方案关键法定日期（只读投影；标签用中文，日期 ISO YYYY-MM-DD）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ActivePlanKeyDate {
    pub label: String,
    pub date: String,
}

/// 当前未完成方案视图（分红/送转/配股/回购/拆股各：阶段+关键日期）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ActiveCorporatePlanView {
    pub kind: CorporateActionKind,
    /// 方案身份：现金分红为 plan_id，其余为 event_id。
    pub identity: String,
    pub stage: ActivePlanStage,
    pub key_dates: Vec<ActivePlanKeyDate>,
}

/// 各行为业务条件快照：`blockers` 为空表示按当前事实可评估的前置条件满足。
/// 「满足」不承诺受理——正式受理仍执行完整制度校验（A 股语义门禁）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct CorporateActionReadiness {
    pub kind: CorporateActionKind,
    pub ready: bool,
    pub blockers: Vec<String>,
}

/// 缴款窗口相对查询日的状态（配股专用；其余行为为窗口自身日期）。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub enum PaymentWindowState {
    BeforeOpen,
    Open,
    Closed,
}

/// 本人（按账户）在某未完成配股方案中的权利/额度摘要（能力面嵌入版；
/// 完整明细见 `session::corporate_actions::OwnerRightsOfferingView`）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct OwnerRightsSummaryView {
    pub event_id: String,
    pub stock: String,
    pub stage: ActivePlanStage,
    pub payment_window: PaymentWindowState,
    /// 本人具名权利股数（无具名权利时 `None`；规范 u64 十进制字符串）。
    pub entitled_shares: Option<String>,
    /// 公开配售剩余额度（规范 u64 十进制字符串）：权证已派发且本人无具名
    /// 权利且方案有公开额度时给出；`Some("0")` 表示额度已用尽（与
    /// `OwnerRightsOfferingView::open_subscription_remaining_shares` 同口径），
    /// 方案无公开额度或已有具名权利时 `None`。
    pub open_subscription_remaining: Option<String>,
}

/// 公司侧当前事实（`CompanySystem::company_facts` 的返回；session 层据此
/// 组装完整 [`CompanyCapabilities`]）。静态布尔位语义与原能力面一致。
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompanyFacts {
    pub revenue: bool,
    pub net_income: bool,
    pub equity: bool,
    pub cash_flow: bool,
    pub full_financial_statements: bool,
    pub cash_settlement: bool,
    pub unsupported_reason: String,
    pub par_value_per_share: CapabilityMoney,
    pub issued_shares: u64,
    pub registered_capital: CapabilityAmount,
    pub distributable_profit: DistributableProfitSnapshot,
}

/// 完整消费面能力视图（owner 隔离：`owner_rights` 只含查询账户本人的事实；
/// 由 session 层组装，company 域不产出半填充视图）。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct CompanyCapabilities {
    // —— 静态能力位（原字段语义不变）——
    pub revenue: bool,
    pub net_income: bool,
    pub equity: bool,
    pub cash_flow: bool,
    pub full_financial_statements: bool,
    pub cash_settlement: bool,
    pub unsupported_reason: String,
    // —— 公司侧当前事实 ——
    pub par_value_per_share: CapabilityMoney,
    /// 现行总股本（规范 u64 十进制字符串；与 `OwnerEntitlementDetail::rights_shares`
    /// 同口径：ts 类型与 serde 实际输出必须同时为字符串，缺一即 wire 断裂）。
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    #[ts(type = "string")]
    pub issued_shares: u64,
    pub registered_capital: CapabilityAmount,
    pub distributable_profit: DistributableProfitSnapshot,
    // —— session 侧补充（方案/条件/本人）——
    /// 当前未完成方案（含各阶段；空列表 = 确无未完成方案）。
    pub active_plans: Vec<ActiveCorporatePlanView>,
    /// 各行为业务条件快照（五类各一条，顺序固定）。
    pub action_readiness: Vec<CorporateActionReadiness>,
    /// 本人（按查询账户）在未完成配股方案中的权利/额度摘要。
    pub owner_rights: Vec<OwnerRightsSummaryView>,
}
