//! 合并域统一错误（K3，任务 12）。绝不静默吞掉（铁律二）：每个变体携带
//! 定位与数值上下文；无子公司是类型化 `NotApplicable`（不是伪装的空合并）。

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::ledger::{AccountElement, LedgerAccountId};
use crate::accounting::period::AccountingPeriod;
use thiserror::Error;

use super::group::MemberId;

/// 合并操作失败。
#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum ConsolidationError {
    /// 公司没有子公司：合并范围不适用（K3 要求显式 `NotApplicable`，不返回
    /// 伪空合并结果）。
    #[error("consolidation not applicable: company {company:?} has no subsidiary")]
    NotApplicable { company: MemberId },

    /// 同一成员在集团内出现两次（重复合并会把账套双计）。
    #[error("duplicate group member {member:?}")]
    DuplicateMember { member: MemberId },

    /// 请求的合并根不在成员清单内。
    #[error("unknown consolidation root {root:?}")]
    UnknownRoot { root: MemberId },

    /// 成员声明的固定母公司不在成员清单内。
    #[error("member {member:?} declares unknown group parent {parent:?}")]
    UnknownGroupParent { member: MemberId, parent: MemberId },

    /// 集团链成环（沿 group_parent 走回已访问成员）。
    #[error("group graph has a cycle through member {member:?}")]
    GroupCycle { member: MemberId },

    /// 合并根本身有母公司（不是顶级成员）。
    #[error("consolidation root {root:?} itself has parent {parent:?}")]
    RootHasParent { root: MemberId, parent: MemberId },

    /// 成员的母公司链终止于另一个顶级成员：不属于请求合并的集团。
    #[error("member {member:?} belongs to group of {top:?}, not requested root {root:?}")]
    MemberOutsideGroup {
        member: MemberId,
        top: MemberId,
        root: MemberId,
    },

    /// 多层集团（子公司的子公司）：固定单层合并模型显式不支持，不静默
    /// 近似（多层需要间接持股乘积，超出本任务范围）。
    #[error(
        "nested group unsupported: member {member:?} is a subsidiary of {parent:?} (only direct subsidiaries of the root are consolidatable)"
    )]
    NestedGroupUnsupported { member: MemberId, parent: MemberId },

    /// 合并根持有自身股权申报（根无母公司，持股字段必须为零）。
    #[error("consolidation root {root:?} declares parent-held shares {parent_held_shares}")]
    RootWithHolding {
        root: MemberId,
        parent_held_shares: u64,
    },

    /// 子公司申报零持股。
    #[error("member {member:?} declares zero parent-held shares")]
    ZeroHolding { member: MemberId },

    /// 持股数超过已发行股份总数。
    #[error("member {member:?} holding {held} exceeds issued shares {issued}")]
    HoldingBeyondIssued {
        member: MemberId,
        held: u64,
        issued: u64,
    },

    /// 持股比例不能精确表示为整数基点（held×10000 不能被 issued 整除）：
    /// 显式拒绝，绝不静默舍入（合并拆分必须分毫不差）。
    #[error("ownership of {member:?} ({held}/{issued}) is not an exact basis-point ratio")]
    OwnershipNotRepresentable {
        member: MemberId,
        held: u64,
        issued: u64,
    },

    /// 控制与持股不一致：固定控制关系要求母公司持股严格大于 5000bp
    ///（恰好半数不构成控制）。
    #[error(
        "control-ownership inconsistency: member {member:?} parent holds {ownership_bp} bp (must be > 5000)"
    )]
    ControlOwnershipMismatch { member: MemberId, ownership_bp: i64 },

    /// 成员间会计期间覆盖不一致（固定集团要求同期间记账）。
    #[error("period coverage mismatch for {member:?}: expected {expected:?}, actual {actual:?}")]
    PeriodCoverageMismatch {
        member: MemberId,
        expected: Vec<AccountingPeriod>,
        actual: Vec<AccountingPeriod>,
    },

    /// 同一科目代码在不同成员科目表中语义冲突（要素/现金/备抵任一不同）：
    /// 跨成员按代码加总的前提被破坏，必须显式失败。
    #[error(
        "chart conflict on account {code}: {member_a:?} ({detail_a}) vs {member_b:?} ({detail_b})"
    )]
    ChartConflict {
        code: LedgerAccountId,
        member_a: MemberId,
        detail_a: String,
        member_b: MemberId,
        detail_b: String,
    },

    /// 内部交易申报引用了不在集团内的成员。
    #[error("intercompany declaration references unknown member {member:?}")]
    UnknownIntercompanyMember { member: MemberId },

    /// 成员对自己申报内部交易。
    #[error("intercompany declaration references itself: {member:?}")]
    IntercompanySelfReference { member: MemberId },

    /// 往来抵销的两侧要素不成「一资产一负债」对。
    #[error(
        "intercompany pair shape invalid: {member_a:?} account {account_a:?} is {element_a:?}, {member_b:?} account {account_b:?} is {element_b:?} (need one Asset and one Liability)"
    )]
    IntercompanyPairShape {
        member_a: MemberId,
        account_a: LedgerAccountId,
        element_a: AccountElement,
        member_b: MemberId,
        account_b: LedgerAccountId,
        element_b: AccountElement,
    },

    /// 对手方余额不符（或单侧缺失）：列出两侧数值，绝不用差额 plug 平账。
    /// `side_b` 装箱控制错误体积（clippy::result_large_err；错误路径非热路径）。
    #[error("intercompany counterparty mismatch: {side_a:?} vs {side_b:?} (no plug balancing)")]
    CounterpartyMismatch {
        side_a: DeclaredSide,
        side_b: Option<Box<DeclaredSide>>,
    },

    /// 申报科目是现金类：工作底稿分录绝不触碰现金（集团现金不变的红线）。
    #[error(
        "intercompany declaration touches cash account {account:?} of {member:?}: worksheet entries must never touch cash"
    )]
    IntercompanyTouchesCash {
        member: MemberId,
        account: LedgerAccountId,
    },

    /// 申报科目不存在于对应成员的科目表。
    #[error("intercompany account {account:?} not in chart of {member:?}")]
    UnknownIntercompanyAccount {
        member: MemberId,
        account: LedgerAccountId,
    },

    /// 内部销售转移价非正数。
    #[error(
        "intercompany sale invoice must be positive: {invoice} (seller {seller:?}, buyer {buyer:?})"
    )]
    SaleInvoiceNotPositive {
        seller: MemberId,
        buyer: MemberId,
        invoice: AccountingAmount,
    },

    /// 内部销售卖方成本高于转移价（亏损内部交易）：未支持的简化边界。
    #[error(
        "intercompany sale cost {cost} exceeds invoice {invoice} (seller {seller:?}, buyer {buyer:?}); below-cost internal sale unsupported"
    )]
    SaleCostBeyondInvoice {
        seller: MemberId,
        buyer: MemberId,
        invoice: AccountingAmount,
        cost: AccountingAmount,
    },

    /// 未售存货申报超过转移价总额（买方账面不可能超过全部购入额）。
    #[error(
        "intercompany sale unsold inventory {unsold} exceeds invoice {invoice} (seller {seller:?}, buyer {buyer:?})"
    )]
    SaleUnsoldBeyondInvoice {
        seller: MemberId,
        buyer: MemberId,
        invoice: AccountingAmount,
        unsold: AccountingAmount,
    },

    /// 销售抵销科目的要素与申报不符（收入须 Revenue、成本须 Expense、
    /// 买方存货须非现金 Asset）。
    #[error(
        "intercompany sale account {account:?} of {member:?} is {actual:?}, expected {expected:?}"
    )]
    SaleAccountElement {
        member: MemberId,
        account: LedgerAccountId,
        expected: AccountElement,
        actual: AccountElement,
    },

    /// 底层会计运算失败（checked i128 溢出等，原样透传）。
    #[error(transparent)]
    Accounting(#[from] AccountingError),
}

/// 一侧已申报的往来余额（错误上下文用）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct DeclaredSide {
    pub member: MemberId,
    pub account: LedgerAccountId,
    pub amount: AccountingAmount,
}

impl std::fmt::Display for DeclaredSide {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:?} account {} amount {}",
            self.member, self.account, self.amount
        )
    }
}
