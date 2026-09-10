//! 工商域统一错误（K3）。绝不静默吞错（铁律二）：每个变体携带定位与数值
//! 上下文。`PaymentFailed` 是账套负现金禁令（K2）在经营域的类型化映射——
//! 资金不足不是引擎错误：公司继续运行，无透支、无自动补钱。

use crate::accounting::{
    AccountingAmount, AccountingError, FixedAssetError, InventoryError, InventoryItemCode,
    LedgerAccountId, OpenItemId, TaxPolicyError, TradeLedgerError,
};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::counterparty::CounterpartyId;
use crate::company::CompanyError;
use thiserror::Error;

#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum IndustrialError {
    /// 付款将打负现金（`NegativeCashProhibited` 的领域映射）：类型化拒绝，
    /// 账套与子账零改动；逾期/欠款以 Overdue 开项面呈现（不补钱）。
    #[error("payment failed (insufficient cash): {source}")]
    PaymentFailed { source: AccountingError },

    #[error("issue {requested} of item {item:?} exceeds on-hand {available}")]
    InsufficientInventory {
        item: InventoryItemCode,
        requested: i128,
        available: i128,
    },

    #[error("{what} must be positive, got {amount:?}")]
    NonPositiveAmount {
        what: &'static str,
        amount: AccountingAmount,
    },

    #[error("due date {due_on} is before business date {date}")]
    InvalidDueDate { due_on: CivilDate, date: CivilDate },

    #[error("rate {rate_bp}bp out of [0, 10000]")]
    InvalidRateBp { rate_bp: i32 },

    /// 结税/付息无应处理金额（显式拒绝而非静默 no-op）。
    #[error("nothing to pay")]
    NothingToPay,

    #[error("contract {contract:?} has no accrued unpaid interest")]
    NothingAccrued { contract: ContractId },

    #[error(
        "accrual through {through} is not after last accrual {last_accrual} (contract {contract:?})"
    )]
    AccrualNotForward {
        contract: ContractId,
        through: CivilDate,
        last_accrual: CivilDate,
    },

    #[error("unknown loan contract {contract:?}")]
    UnknownLoan { contract: ContractId },

    #[error("repay {requested:?} exceeds outstanding {outstanding:?} of contract {contract:?}")]
    PrincipalBeyondOutstanding {
        contract: ContractId,
        requested: AccountingAmount,
        outstanding: AccountingAmount,
    },

    #[error("income tax payment {requested:?} exceeds payable {payable:?}")]
    TaxOverpayment {
        requested: AccountingAmount,
        payable: AccountingAmount,
    },

    #[error(
        "write-off of {id:?} (open {open:?}) exceeds bad-debt allowance {allowance:?}; accrue first"
    )]
    InsufficientAllowance {
        id: OpenItemId,
        allowance: AccountingAmount,
        open: AccountingAmount,
    },

    #[error(
        "no credit line with {lender:?}; borrowing {requested:?} rejected (no infinite credit)"
    )]
    NoCreditLine {
        lender: CounterpartyId,
        requested: AccountingAmount,
    },

    #[error(
        "borrowing {requested:?} beyond credit line {limit:?} (outstanding {outstanding:?}, lender {lender:?}; opening debt included)"
    )]
    DebtBeyondCreditLine {
        lender: CounterpartyId,
        outstanding: AccountingAmount,
        requested: AccountingAmount,
        limit: AccountingAmount,
    },

    #[error("opening debt terms say {configured:?} but ledger 2001 credit balance is {ledger:?}")]
    OpeningDebtMismatch {
        ledger: AccountingAmount,
        configured: AccountingAmount,
    },

    #[error(
        "opening inventory seed says {seeded:?} but ledger account {account} holds {ledger:?}"
    )]
    OpeningSeedMismatch {
        account: LedgerAccountId,
        ledger: AccountingAmount,
        seeded: AccountingAmount,
    },

    /// 开局带累计折旧暂不支持（诚实边界）：经营前史由任务 14 用同一处理器生成。
    #[error(
        "opening accumulated depreciation (1602 = {balance:?}) unsupported; generate prehistory via task 14"
    )]
    OpeningAccumulatedDepreciation { balance: AccountingAmount },

    #[error(transparent)]
    Inventory(#[from] InventoryError),

    #[error(transparent)]
    Asset(#[from] FixedAssetError),

    #[error(transparent)]
    Trade(#[from] TradeLedgerError),

    #[error(transparent)]
    Tax(#[from] TaxPolicyError),

    #[error(transparent)]
    Company(#[from] CompanyError),

    #[error(transparent)]
    Accounting(#[from] AccountingError),
}

/// 过账错误 → 领域错误映射：批末负现金（`NegativeCashProhibited` 的领域包装，
/// K2 负现金禁令）→ `PaymentFailed`；其余会计错误原样透传（不吞错）。
pub(super) fn map_post_error(source: AccountingError) -> IndustrialError {
    if let AccountingError::BatchAborted { cause, .. } = &source {
        if matches!(**cause, AccountingError::NegativeCashProhibited { .. }) {
            return IndustrialError::PaymentFailed { source };
        }
    }
    IndustrialError::Accounting(source)
}

/// 存货发出类错误 → 经营域映射（未知项目 = 0 可用；保持携带请求/可用上下文）。
pub(super) fn map_inventory_issue(
    quantity: i128,
    error: crate::accounting::InventoryError,
) -> IndustrialError {
    match error {
        crate::accounting::InventoryError::InsufficientQuantity {
            item,
            requested,
            available,
        } => IndustrialError::InsufficientInventory {
            item,
            requested,
            available,
        },
        crate::accounting::InventoryError::UnknownItem { item } => {
            IndustrialError::InsufficientInventory {
                item,
                requested: quantity,
                available: 0,
            }
        }
        other => IndustrialError::Inventory(other),
    }
}
