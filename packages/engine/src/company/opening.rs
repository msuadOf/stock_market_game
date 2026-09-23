//! 开局账套（K2）：显式平衡的 OpeningBalance 凭证 + 行业子账占位类型。
//!
//! 开局**不是**从最新股价或旧 V 反推的资产（K2 红线）：每一行都是显式的虚构
//! 配置，平衡由 [`Books::post_batch`] 的复式验证强制（不平衡 ⇒
//! `CompanyError::OpeningPost`，账套零改动）。2 个完整自然年度的经营前史由
//! 任务 14 用同一会计处理器生成，历史发布集装配在任务 15；本模块只锚定
//! `as_of`（开局前一自然日）与期初凭证。
//!
//! [`Books::post_batch`]: crate::accounting::Books::post_batch

use crate::accounting::{
    AccountChart, AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, JournalEntry,
    JournalLine, LedgerAccountId, PostingSide,
};
use crate::calendar::CivilDate;

/// 开局凭证的业务事件来源 id。每公司账套内唯一（各自持有独立 `Books`）；
/// 恒为 1，远小于 2^53（JSON number 安全域，任务 6 review 注记）。
pub fn opening_event_id() -> BusinessEventId {
    BusinessEventId::new(1)
}

/// 开局分录行（值对象）：字段公开，非法草稿是可表示输入，过账是唯一验证边界。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct OpeningLine {
    pub account: LedgerAccountId,
    pub side: PostingSide,
    pub amount: AccountingAmount,
}

/// 公司开局账套：版本化科目表 + 锚点日 + 显式平衡的期初行。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct CompanyOpening {
    pub chart: AccountChart,
    /// 期初余额锚点日（开局前一自然日；经营前史在任务 14 回填此日之前）。
    pub as_of: CivilDate,
    pub lines: Vec<OpeningLine>,
}

impl CompanyOpening {
    /// 通用 v1 科目表上的开局账套（行业科目表在任务 8–11 以新版本扩充，
    /// 不改动通用表语义）。
    pub fn generic_chart(as_of: CivilDate, lines: Vec<OpeningLine>) -> Self {
        Self {
            chart: AccountChart::generic_v1(),
            as_of,
            lines,
        }
    }

    /// 期初余额凭证。现金流类别 = `Financing`：期初投入计入其所属期间的筹资
    /// 口径（与任务 6 金样的 2029-12 期初处理一致）。
    pub fn to_voucher(&self) -> JournalEntry {
        JournalEntry {
            source: opening_event_id(),
            date: self.as_of,
            kind: BusinessKind::OpeningBalance,
            cash_flow: CashFlowClass::Financing,
            lines: self
                .lines
                .iter()
                .map(|line| JournalLine {
                    account: line.account.clone(),
                    side: line.side,
                    amount: line.amount,
                })
                .collect(),
        }
    }
}

/// 公司子账占位集合：行业子账内容在任务 8–11 填充，这里固定账套结构
/// （合同/资产/库存三类子账与总账同存同档）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SubsidiaryLedgers {
    /// 合同子账（履约/计价明细；任务 8–11）。
    pub contracts: ContractSubLedger,
    /// 资产子账（固定资产登记与折旧明细；任务 8）。
    pub assets: AssetSubLedger,
    /// 库存子账（数量与移动加权平均成本；任务 8）。
    pub inventory: InventorySubLedger,
}

/// 合同子账占位（任务 8–11 提供行业内容）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct ContractSubLedger;

/// 资产子账占位（任务 8 提供固定资产登记内容）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AssetSubLedger;

/// 库存子账占位（任务 8 提供存货数量/成本内容）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct InventorySubLedger;
