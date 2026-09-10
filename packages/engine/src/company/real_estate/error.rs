//! 地产域统一错误（K3，任务 11）。绝不静默吞错（铁律二）：每个变体携带定位
//! 与数值上下文。`PaymentFailed` 是账套负现金禁令（K2）在经营域的类型化映射
//! ——资金不足不是引擎错误：公司继续运行，无透支、无自动补钱。

use crate::accounting::{AccountingAmount, AccountingError, LedgerAccountId};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::counterparty::CounterpartyId;
use crate::company::real_estate::projects::ProjectId;
use crate::company::CompanyError;
use thiserror::Error;

#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum RealEstateError {
    /// 付款将打负现金（`NegativeCashProhibited` 的领域映射）：类型化拒绝，
    /// 账套与子账零改动；无透支、无自动补钱（K2）。
    #[error("payment failed (insufficient cash): {source}")]
    PaymentFailed { source: AccountingError },

    #[error("{what} must be positive, got {amount:?}")]
    NonPositiveAmount {
        what: &'static str,
        amount: AccountingAmount,
    },

    #[error("units must be >= 1, got {units}")]
    NonPositiveUnits { units: i128 },

    #[error("unknown project {project:?}")]
    UnknownProject { project: ProjectId },

    #[error("unknown presale contract {contract:?}")]
    UnknownPresale { contract: ContractId },

    #[error("unknown loan contract {contract:?}")]
    UnknownLoan { contract: ContractId },

    #[error("duplicate project id {project:?}")]
    DuplicateProject { project: ProjectId },

    #[error("duplicate presale contract id {contract:?}")]
    DuplicatePresale { contract: ContractId },

    #[error("duplicate loan contract id {contract:?}")]
    DuplicateContract { contract: ContractId },

    /// 单公司项目数上限（K2 需求约束；上限来自版本化配置）。
    #[error("project count limit {limit} reached; no new projects")]
    ProjectCountLimit { limit: usize },

    #[error("presale of {requested} units on {project:?} exceeds available {available} units")]
    PresaleBeyondAvailableUnits {
        project: ProjectId,
        requested: i128,
        available: i128,
    },

    #[error(
        "presale collection {requested:?} would exceed contract {contract:?} price beyond collected (remaining headroom {remaining:?})"
    )]
    PresaleBeyondContract {
        contract: ContractId,
        requested: AccountingAmount,
        remaining: AccountingAmount,
    },

    /// QA 红线：未达到交付条件（项目未完工）不得确认收入（控制权未转移，
    /// CAS 14 §4/§13——已核验）。
    #[error(
        "delivery of {contract:?} rejected: project {project:?} not completed (control not transferred)"
    )]
    DeliveryBeforeCompletion {
        project: ProjectId,
        contract: ContractId,
    },

    #[error("presale contract {contract:?} already delivered")]
    PresaleAlreadyDelivered { contract: ContractId },

    #[error(
        "presale collection on {contract:?} rejected: contract delivered; collect the final payment receivable instead"
    )]
    CollectionAfterDelivery { contract: ContractId },

    #[error("carry-out of {requested} units exceeds remaining {available} of {project:?}")]
    CarryBeyondRemainingUnits {
        project: ProjectId,
        requested: i128,
        available: i128,
    },

    #[error(
        "accrual through {through} is not after last accrual {last_accrual} (contract {contract:?})"
    )]
    AccrualNotForward {
        contract: ContractId,
        through: CivilDate,
        last_accrual: CivilDate,
    },

    /// 付息无应处理金额（显式拒绝而非静默 no-op）。
    #[error("loan {contract:?} has no accrued unpaid interest")]
    NothingAccrued { contract: ContractId },

    #[error("repay {requested:?} exceeds outstanding {outstanding:?} of contract {contract:?}")]
    PrincipalBeyondOutstanding {
        contract: ContractId,
        requested: AccountingAmount,
        outstanding: AccountingAmount,
    },

    /// 资本化政策/配置非法（阈值、项目上限等）。
    #[error("invalid capitalization/config policy: {detail}")]
    InvalidPolicy { detail: String },

    #[error("suspend rejected: project {project:?} development not started")]
    SuspensionBeforeDevelopment { project: ProjectId },

    #[error("suspend rejected: project {project:?} already completed")]
    SuspensionAfterCompletion { project: ProjectId },

    #[error("suspend rejected: project {project:?} already suspended")]
    AlreadySuspended { project: ProjectId },

    #[error("resume rejected: project {project:?} not suspended")]
    NotSuspended { project: ProjectId },

    #[error("resume {resume_on} not after suspension start {suspended_on} (project {project:?})")]
    ResumeNotForward {
        project: ProjectId,
        suspended_on: CivilDate,
        resume_on: CivilDate,
    },

    #[error("complete rejected: project {project:?} development not started")]
    CompleteBeforeDevelopment { project: ProjectId },

    #[error("complete rejected while suspended: resume project {project:?} first")]
    CompleteWhileSuspended { project: ProjectId },

    #[error("complete rejected: project {project:?} already completed")]
    AlreadyCompleted { project: ProjectId },

    #[error("development spend rejected while suspended: resume project {project:?} first")]
    DevelopmentWhileSuspended { project: ProjectId },

    #[error("development spend rejected: project {project:?} already completed")]
    DevelopmentAfterCompletion { project: ProjectId },

    #[error(
        "no credit line with {lender:?}; borrowing {requested:?} rejected (no infinite credit)"
    )]
    NoCreditLine {
        lender: CounterpartyId,
        requested: AccountingAmount,
    },

    #[error(
        "borrowing {requested:?} beyond credit line {limit:?} (outstanding {outstanding:?}, lender {lender:?})"
    )]
    DebtBeyondCreditLine {
        lender: CounterpartyId,
        outstanding: AccountingAmount,
        requested: AccountingAmount,
        limit: AccountingAmount,
    },

    /// 开局给地产子账科目种子暂不支持（诚实边界：经营前史由任务 14 用同一
    /// 处理器生成，不从存档倒推）。
    #[error(
        "opening lines must not seed real-estate sub-ledger account {account}; prehistory is task 14"
    )]
    OpeningRealEstateSeeded { account: LedgerAccountId },

    #[error(transparent)]
    Trade(#[from] crate::accounting::TradeLedgerError),

    #[error(transparent)]
    Company(#[from] CompanyError),

    #[error(transparent)]
    Accounting(#[from] AccountingError),

    #[error(transparent)]
    Calendar(#[from] crate::calendar::CivilDateError),
}

/// 过账错误 → 领域错误映射：批末负现金（`NegativeCashProhibited` 的领域
/// 包装，K2 负现金禁令）→ `PaymentFailed`；其余会计错误原样透传（不吞错）。
pub(super) fn map_post_error(source: AccountingError) -> RealEstateError {
    if let AccountingError::BatchAborted { cause, .. } = &source {
        if matches!(**cause, AccountingError::NegativeCashProhibited { .. }) {
            return RealEstateError::PaymentFailed { source };
        }
    }
    RealEstateError::Accounting(source)
}
