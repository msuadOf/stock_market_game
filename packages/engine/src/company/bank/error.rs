//! 银行域统一错误（K3，任务 9）。绝不静默吞错（铁律二）：每个变体携带定位
//! 与数值上下文。`PaymentFailed` 是负现金禁令（K2）在银行经营域的类型化映射
//! ——客户流动性约束不足不是引擎错误：银行继续运行（提款失败、头寸保留），
//! 无透支、无自动补钱、无央行兜底。

use super::ecl::EclStage;
use super::BankProductKind;
use crate::accounting::{AccountingAmount, AccountingError, LedgerAccountId};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::CompanyError;
use thiserror::Error;

#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum BankError {
    /// 付款/提款将打负现金（`NegativeCashProhibited` 的领域映射）：类型化
    /// 拒绝，账套与子账零改动（K2 客户流动性约束：银行继续运行）。
    #[error("payment failed (insufficient cash): {source}")]
    PaymentFailed { source: AccountingError },

    /// 未支持的合同/产品种类（docs/company-accounting.md §6：结构化衍生品、
    /// FVTPL/FVOCI 投资等显式不支持，不冒充已实现、不静默 fallback）。
    #[error("unsupported bank product kind {kind:?}: {detail}")]
    UnsupportedContract {
        kind: BankProductKind,
        detail: &'static str,
    },

    #[error("{what} must be positive, got {amount:?}")]
    NonPositiveAmount {
        what: &'static str,
        amount: AccountingAmount,
    },

    #[error("rate {rate_bp}bp must be >= 0")]
    InvalidRateBp { rate_bp: i32 },

    /// ECL 情景/政策非法（PD/LGD/权重越界、权重和 ≠ 10000、空情景表）。
    #[error("invalid ECL input: {detail}")]
    EclInvalid { detail: String },

    #[error("contract {contract:?} maturity {maturity} is not after start {start}")]
    MaturityNotAfterStart {
        contract: ContractId,
        start: CivilDate,
        maturity: CivilDate,
    },

    #[error("unknown deposit contract {contract:?}")]
    UnknownDeposit { contract: ContractId },

    #[error("unknown loan contract {contract:?}")]
    UnknownLoan { contract: ContractId },

    #[error("duplicate contract id {contract:?}")]
    DuplicateContract { contract: ContractId },

    #[error("withdrawal {requested:?} exceeds deposit principal {outstanding:?} of {contract:?}")]
    WithdrawalBeyondPrincipal {
        contract: ContractId,
        requested: AccountingAmount,
        outstanding: AccountingAmount,
    },

    #[error(
        "collection {requested:?} exceeds outstanding principal {outstanding:?} of {contract:?}"
    )]
    PrincipalBeyondOutstanding {
        contract: ContractId,
        requested: AccountingAmount,
        outstanding: AccountingAmount,
    },

    #[error(
        "interest collection {requested:?} exceeds accrued receivable {accrued:?} of {contract:?}"
    )]
    InterestBeyondAccrued {
        contract: ContractId,
        requested: AccountingAmount,
        accrued: AccountingAmount,
    },

    /// 付息无应处理金额（显式拒绝而非静默 no-op）。
    #[error("deposit {contract:?} has no accrued unpaid interest")]
    NothingAccrued { contract: ContractId },

    #[error(
        "accrual through {through} is not after last accrual {last_accrual} (contract {contract:?})"
    )]
    AccrualNotForward {
        contract: ContractId,
        through: CivilDate,
        last_accrual: CivilDate,
    },

    /// 核销要求准备足以覆盖账面余额（先计提足额——中国实务核销条件）。
    #[error(
        "write-off of {contract:?} (gross {gross:?}) exceeds loan loss allowance {allowance:?}; assess credit first"
    )]
    InsufficientAllowance {
        contract: ContractId,
        allowance: AccountingAmount,
        gross: AccountingAmount,
    },

    #[error("loan {contract:?} is already written off")]
    LoanAlreadyWrittenOff { contract: ContractId },

    #[error("loan {contract:?} is not written off (recovery applies to written-off loans only)")]
    LoanNotWrittenOff { contract: ContractId },

    #[error(
        "recovery {requested:?} exceeds remaining recoverable {recoverable:?} of {contract:?}"
    )]
    RecoveryBeyondRecoverable {
        contract: ContractId,
        requested: AccountingAmount,
        recoverable: AccountingAmount,
    },

    /// 已核销贷款不允许阶段转移（阶段语义只属于存续贷款；重估仍允许）。
    #[error("stage transfer to {to_stage:?} rejected: loan {contract:?} is written off")]
    StageTransferOnWrittenOff {
        contract: ContractId,
        to_stage: EclStage,
    },

    /// 开局给银行子账科目种子暂不支持（诚实边界：经营前史由任务 14 用同一
    /// 处理器生成，不从存档倒推）。
    #[error(
        "opening lines must not seed bank sub-ledger account {account}; prehistory is task 14"
    )]
    OpeningBankBooksSeeded { account: LedgerAccountId },

    #[error(transparent)]
    Company(#[from] CompanyError),

    #[error(transparent)]
    Accounting(#[from] AccountingError),
}

/// 过账错误 → 领域错误映射：批末负现金（`NegativeCashProhibited` 的领域
/// 包装，K2 负现金禁令）→ `PaymentFailed`；其余会计错误原样透传（不吞错）。
pub(super) fn map_post_error(source: AccountingError) -> BankError {
    if let AccountingError::BatchAborted { cause, .. } = &source {
        if matches!(**cause, AccountingError::NegativeCashProhibited { .. }) {
            return BankError::PaymentFailed { source };
        }
    }
    BankError::Accounting(source)
}
