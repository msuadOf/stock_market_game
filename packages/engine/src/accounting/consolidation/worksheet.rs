//! 合并工作底稿值类型（K3，任务 12）：内部交易申报 + 抵销分录。
//!
//! 抵销分录属于合并工作底稿，**绝不回记任何成员账套**——`WorksheetLine`
//! 只携带成员/科目/方向/金额，不产生 `JournalEntry`，因此集团现金逐分不变
//! （`集团现金 == Σ 成员现金`），由「申报科目不得为现金」+ 全部抵销科目
//! （应收/应付/收入/成本/存货）天然非现金共同保证。
//!
//! 申报是**调用方派生**的事实（账套总账不带对手方粒度）：任务 13/26 从
//! 行业子账（`TradeOpenLedger` 开项 / 存货批次）派生 `IntercompanyBalance`
//! 与 `IntercompanySale` 后交给 [`super::consolidate`]。

use crate::accounting::amount::AccountingAmount;
use crate::accounting::journal::PostingSide;
use crate::accounting::ledger::LedgerAccountId;

use super::group::MemberId;

/// 内部往来余额申报：`member` 账上因与 `counterparty`（集团成员）的交易
/// 持有的 `account` 余额 `amount`（恒正；方向由科目要素决定）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct IntercompanyBalance {
    pub member: MemberId,
    pub counterparty: MemberId,
    pub account: LedgerAccountId,
    pub amount: AccountingAmount,
}

/// 内部商品交易申报（赊销口径；应收/应付抵销由 `IntercompanyBalance` 单独
/// 申报）。金额语义：`invoice_amount` = 转移价总额（卖方收入）；
/// `cost_amount` = 对应卖方成本；`unsold_inventory` = 期末买方账面仍未售出
/// 的该批存货（转移价口径，须 ≤ invoice_amount）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct IntercompanySale {
    pub seller: MemberId,
    pub buyer: MemberId,
    pub revenue_account: LedgerAccountId,
    pub cost_account: LedgerAccountId,
    pub inventory_account: LedgerAccountId,
    pub invoice_amount: AccountingAmount,
    pub cost_amount: AccountingAmount,
    pub unsold_inventory: AccountingAmount,
}

/// 工作底稿分录的成因标签。
#[derive(Copy, Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum WorksheetReason {
    /// 内部往来（应收/应付）抵销。
    IntercompanyBalance,
    /// 内部销售与未实现利润抵销。
    IntercompanySale,
}

/// 工作底稿分录行：归属成员 + 科目 + 方向 + 正金额。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct WorksheetLine {
    pub member: MemberId,
    pub account: LedgerAccountId,
    pub side: PostingSide,
    pub amount: AccountingAmount,
}

/// 工作底稿抵销分录（借贷自平衡；零金额行不产生）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct WorksheetEntry {
    pub reason: WorksheetReason,
    pub lines: Vec<WorksheetLine>,
}
