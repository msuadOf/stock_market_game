//! 公司域统一错误（K2）。绝不静默吞错（铁律二）：每个变体携带定位与数值上下文。

use crate::account::StockCode;
use crate::accounting::AccountingAmount;
use crate::accounting::AccountingError;
use crate::calendar::CivilDate;
use crate::company::contracts::ContractId;
use crate::company::counterparty::CounterpartyId;
use crate::company::spec::CompanyId;
use thiserror::Error;

/// 公司域操作失败（规格/开局/映射/对手方/合同/授信）。
#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum CompanyError {
    /// 规格字段非法（空 id/名称/行业、零股本等）。
    #[error("invalid company spec: {detail}")]
    SpecInvalid { detail: String },

    /// 公司 id 重复。
    #[error("duplicate company id {company:?}")]
    DuplicateCompanyId { company: CompanyId },

    /// 两家公司映射同一发行股票。
    #[error("duplicate issuer stock {stock:?}: company {company:?} after {first:?}")]
    DuplicateIssuerStock {
        stock: StockCode,
        company: CompanyId,
        first: CompanyId,
    },

    /// 集团母公司不在注册表内。
    #[error("company {company:?} references unknown group parent {parent:?}")]
    UnknownGroupParent {
        company: CompanyId,
        parent: CompanyId,
    },

    /// 固定集团关系成环。
    #[error("group parent chain cycles at company {company:?}")]
    GroupCycle { company: CompanyId },

    /// 上市公司映射的股票不在清单内。
    #[error("company {company:?} maps unknown issuer stock {stock:?}")]
    UnknownIssuerStock {
        company: CompanyId,
        stock: StockCode,
    },

    /// 清单内股票没有发行人（发行映射必须完整覆盖清单）。
    #[error("stock {stock:?} has no issuing company")]
    UnmappedStock { stock: StockCode },

    /// 股本不精确匹配：公司已发行股数 ≠ 股票总股本（K2：固定股本与股票总股本一致）。
    #[error(
        "company {company:?} issued {issued_shares} shares but stock {stock:?} has {total_shares}"
    )]
    IssuedSharesMismatch {
        company: CompanyId,
        stock: StockCode,
        issued_shares: u64,
        total_shares: u64,
    },

    /// 开局凭证过账失败（不平衡/未知科目/负现金等；含完整会计上下文）。
    #[error("opening balance rejected for company {company:?}: {source}")]
    OpeningPost {
        company: CompanyId,
        source: AccountingError,
    },

    /// 对手方字段非法（空 id/名称）。
    #[error("invalid counterparty: {detail}")]
    CounterpartyInvalid { detail: String },

    /// 对手方 id 重复。
    #[error("duplicate counterparty {counterparty:?}")]
    DuplicateCounterparty { counterparty: CounterpartyId },

    /// 引用未登记的对手方。
    #[error("unknown counterparty {counterparty:?}")]
    UnknownCounterparty { counterparty: CounterpartyId },

    /// 收付金额非正（方向由收支表示，金额恒正）。
    #[error("flow for {counterparty:?} has non-positive amount {amount:?}")]
    FlowAmountNotPositive {
        counterparty: CounterpartyId,
        amount: AccountingAmount,
    },

    /// 合同本金非正。
    #[error("contract {contract:?} has non-positive principal {principal:?}")]
    NonPositivePrincipal {
        contract: ContractId,
        principal: AccountingAmount,
    },

    /// 合同利率为负（只支持固定非负利率）。
    #[error("contract {contract:?} has negative rate {rate_bp}bp")]
    NegativeRate { contract: ContractId, rate_bp: i32 },

    /// 到期日不晚于起息日。
    #[error("contract {contract:?} maturity {maturity} is not after start {start}")]
    MaturityNotAfterStart {
        contract: ContractId,
        start: CivilDate,
        maturity: CivilDate,
    },

    /// 合同 id 重复。
    #[error("duplicate contract {contract:?}")]
    DuplicateContract { contract: ContractId },

    /// 经营预算字段非法（负现金下限等）。
    #[error("invalid operating budget: {detail}")]
    BudgetInvalid { detail: String },

    /// 授信额度非法（非正上限）。
    #[error("credit line for {lender:?} invalid (limit {limit:?}): {reason}")]
    CreditLineInvalid {
        lender: CounterpartyId,
        limit: AccountingAmount,
        reason: &'static str,
    },

    /// 同一贷款人重复授信。
    #[error("duplicate credit line for lender {lender:?}")]
    DuplicateCreditLine { lender: CounterpartyId },

    /// 无授信仍新增借款（K2：借款必须经额度约束，不允许无限信用兜底）。
    #[error(
        "company {company:?} has no credit line with lender {lender:?}; borrowing {requested} rejected"
    )]
    NoCreditLine {
        company: CompanyId,
        lender: CounterpartyId,
        requested: AccountingAmount,
    },

    /// 新增借款超出授信（携带占用/申请/上限全量上下文）。
    #[error(
        "company {company:?} borrowing {requested} beyond credit line {credit_limit:?} (outstanding {outstanding:?}, lender {lender:?})"
    )]
    DebtBeyondCreditLine {
        company: CompanyId,
        lender: CounterpartyId,
        credit_limit: AccountingAmount,
        outstanding: AccountingAmount,
        requested: AccountingAmount,
    },

    /// 会计底座错误透传（checked 金额运算溢出等）。
    #[error(transparent)]
    Accounting(#[from] AccountingError),
}
