//! 经营编排错误（K4，任务 14）。绝不静默吞错（铁律二）：行业处理器错误原样
//! 透传（不吞、不映射成泛型字符串）；经营层自有守卫（日期序、装配一致性、
//! 前史边界、注入校验）单独建模。`PaymentFailed` **不是**这里的概念——
//! 它在流内被捕获为业务状态（[`crate::company::operations::PaymentFailureRecord`]），
//! 不上抛、不打断经营日。

use thiserror::Error;

/// 注意：不含 `Clone/Eq/PartialEq`（透传的 `CalendarError` 未实现；断言用
/// `matches!` 字段绑定——observation 套件先例）。
#[derive(Debug, Error)]
pub enum OperationsError {
    #[error(transparent)]
    Industrial(#[from] crate::company::industrial::IndustrialError),
    #[error(transparent)]
    Bank(#[from] crate::company::bank::BankError),
    #[error(transparent)]
    Insurance(#[from] crate::company::insurance::InsuranceError),
    #[error(transparent)]
    RealEstate(#[from] crate::company::real_estate::RealEstateError),
    #[error(transparent)]
    Scheduler(#[from] crate::company::scheduler::SchedulerError),
    #[error("date arithmetic error: {0}")]
    Date(#[from] crate::calendar::CivilDateError),
    #[error("calendar error: {0}")]
    Calendar(#[from] crate::calendar::CalendarError),
    #[error("accounting error: {0}")]
    Accounting(#[from] crate::accounting::AccountingError),
    #[error("company error: {0}")]
    Company(#[from] crate::company::CompanyError),

    /// 经营日必须逐日推进（重复/回拨/跳日）。
    #[error("advance to {got} out of sequence: next expected {expected}")]
    DateOutOfSequence {
        expected: crate::calendar::CivilDate,
        got: crate::calendar::CivilDate,
    },

    /// 公司会计类型与经营流参数不匹配（装配错误）。
    #[error("company {company:?} kind {kind:?} does not match flow variant {flow:?}")]
    KindFlowMismatch {
        company: crate::company::spec::CompanyId,
        kind: crate::company::spec::CompanyKind,
        flow: &'static str,
    },

    /// 冲击参数非法（版本化游戏假设的构造校验）。
    #[error("invalid shock params: {detail}")]
    ShockParamsInvalid { detail: String },

    /// 注入的冲击变体与目标槽位不匹配（市场位只收市场冲击等）。
    #[error("shock kind {actual:?} does not fit the {slot} slot")]
    ShockKindMismatch {
        actual: crate::company::events::ShockKind,
        slot: &'static str,
    },

    /// 冲击窗口非法（到期日早于起始日）。
    #[error("shock window invalid: expires {expires_on} before starts {starts_on}")]
    InvalidShockWindow {
        starts_on: crate::calendar::CivilDate,
        expires_on: crate::calendar::CivilDate,
    },

    /// 前史起点早于初始化专用下界（1998-01-01；不用邻年顶替）。
    #[error(
        "prehistory start {requested} precedes init-only floor {floor} (earliest runtime start is 2000-01-01)"
    )]
    HistoryBeforeInitFloor {
        requested: crate::calendar::CivilDate,
        floor: crate::calendar::CivilDate,
    },

    /// 未知到期引用前缀（`AR:`/`LN:`/`DL:` 之外）。
    #[error("unknown maturity reference {reference:?} for company {company:?}")]
    UnknownMaturityReference {
        company: crate::company::spec::CompanyId,
        reference: String,
    },

    /// 流参数给了负的天数偏移（信用期/期限/交付延迟必须 ≥ 0）。
    #[error("negative day offset {days} is not a valid term/credit/lag")]
    NegativeDayOffset { days: i64 },

    /// 经营节奏中的期限/间隔必须至少为一个自然日。
    #[error(
        "company {company:?} has invalid {parameter} duration {days}; expected at least 1 day"
    )]
    InvalidDuration {
        company: crate::company::spec::CompanyId,
        parameter: &'static str,
        days: i64,
    },

    /// 保险账套没有计息承载面（无借款模型）——利息 due 不应注册到保险公司。
    #[error("interest accrual due submitted for insurance company {company:?} (no debt model)")]
    InterestAccrualWithoutDebtModel {
        company: crate::company::spec::CompanyId,
    },
}
