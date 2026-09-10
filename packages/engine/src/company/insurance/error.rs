//! 保险域统一错误（K3，任务 10）。绝不静默吞错（铁律二）：每个变体携带定位
//! 与数值上下文。`PaymentFailed` 是负现金禁令（K2）在保险经营域的类型化映射
//! ——赔款支付超可支付现金不是引擎错误：险企继续运行（支付失败、负债保留），
//! 无透支、无自动补钱。

use super::claims::ClaimId;
use super::InsuranceProductKind;
use crate::accounting::{AccountingAmount, AccountingError, LedgerAccountId};
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::CompanyError;
use thiserror::Error;

#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum InsuranceError {
    /// 赔款支付将打负现金（`NegativeCashProhibited` 的领域映射）：类型化
    /// 拒绝，账套与子账零改动（K2 客户流动性约束：险企继续运行）。
    #[error("payment failed (insufficient cash): {source}")]
    PaymentFailed { source: AccountingError },

    /// 未支持的合同/产品种类（docs/company-accounting.md §6：分红/投连/
    /// 再保险、保费分配法/浮动收费法显式不支持，不冒充已实现）。
    #[error("unsupported insurance product kind {kind:?}: {detail}")]
    UnsupportedContract {
        kind: InsuranceProductKind,
        detail: &'static str,
    },

    #[error("{what} must be positive (or zero where allowed), got {amount:?}")]
    NonPositiveAmount {
        what: &'static str,
        amount: AccountingAmount,
    },

    /// 贴现假设非法：利率必须为正（0 也不接受——不虚构无贴现计量）且
    /// ≤ 10000bp；版本化显式配置（游戏假设）。
    #[error("discount rate {rate_bp}bp out of (0, 10000]")]
    InvalidDiscountRate { rate_bp: i32 },

    #[error("group {group:?} coverage end {end} is not after start {start}")]
    CoverageEndNotAfterStart {
        group: ContractId,
        start: CivilDate,
        end: CivilDate,
    },

    #[error("unknown contract group {group:?}")]
    UnknownGroup { group: ContractId },

    #[error("duplicate contract group {group:?}")]
    DuplicateGroup { group: ContractId },

    #[error("duplicate claim {claim:?} in group {group:?}")]
    DuplicateClaim { group: ContractId, claim: ClaimId },

    #[error("unknown claim {claim:?} in group {group:?}")]
    UnknownClaim { group: ContractId, claim: ClaimId },

    #[error("service units {units} must be positive (negative/zero rejected)")]
    NonPositiveServiceUnits { units: i64 },

    #[error("release of {requested} units exceeds remaining coverage {remaining} of {group:?}")]
    ServiceUnitsBeyondCoverage {
        group: ContractId,
        requested: i64,
        remaining: i64,
    },

    #[error("premium collection {requested:?} exceeds receivable {outstanding:?} of {group:?}")]
    PremiumBeyondReceivable {
        group: ContractId,
        requested: AccountingAmount,
        outstanding: AccountingAmount,
    },

    #[error("claim payment {requested:?} exceeds unpaid {outstanding:?} of {claim:?}")]
    ClaimPaymentBeyondOutstanding {
        claim: ClaimId,
        requested: AccountingAmount,
        outstanding: AccountingAmount,
    },

    /// 保障期已尽后不得再引入新的剩余预期赔付（重估只针对剩余责任）。
    #[error("group {group:?} has no remaining coverage units; cannot set estimate {estimate:?}")]
    NoRemainingCoverage {
        group: ContractId,
        estimate: AccountingAmount,
    },

    /// 开局给保险子账科目种子暂不支持（诚实边界：经营前史由任务 14 用同一
    /// 处理器生成，不从存档倒推）。
    #[error(
        "opening lines must not seed insurance sub-ledger account {account}; prehistory is task 14"
    )]
    OpeningInsuranceBooksSeeded { account: LedgerAccountId },

    #[error(transparent)]
    Company(#[from] CompanyError),

    #[error(transparent)]
    Accounting(#[from] AccountingError),
}

/// 过账错误 → 领域错误映射：批末负现金（`NegativeCashProhibited` 的领域
/// 包装，K2 负现金禁令）→ `PaymentFailed`；其余会计错误原样透传（不吞错）。
pub(super) fn map_post_error(source: AccountingError) -> InsuranceError {
    if let AccountingError::BatchAborted { cause, .. } = &source {
        if matches!(**cause, AccountingError::NegativeCashProhibited { .. }) {
            return InsuranceError::PaymentFailed { source };
        }
    }
    InsuranceError::Accounting(source)
}
