//! K2 权威日记账（分录值类型）：来源唯一性 + 借贷行复式不变量。
//!
//! Journal 状态（批次/来源索引/封账）在 [`state::Journal`]；本文件承载
//! 分录、行、业务种类与现金流类别等值对象及不变量验证。Journal 是**事实**
//! （facts）：随存档保存；余额/索引是派生量，恢复时由 [`super::Books`]
//! 重放重建（来源事实与派生 report 边界不可混用）。每行金额恒正、方向由
//! 借贷表示；负权益/亏损是合法状态，负现金由过账验证拒绝（不 clamp）。
//! 行业业务种类在任务 8–11 扩充。

mod state;

pub use state::Journal;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::period::AccountingPeriod;
use crate::calendar::CivilDate;

/// 业务事件来源 newtype：一个业务事件至多入账一次（K2 来源唯一性）。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
#[serde(transparent)]
#[ts(type = "number")]
pub struct BusinessEventId(u64);

impl BusinessEventId {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn value(self) -> u64 {
        self.0
    }
}

/// 业务种类（分类标签，不驱动过账逻辑；行业事件在任务 8–11 扩充此枚举）。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum BusinessKind {
    /// 期初余额凭证（显式平衡的开业账套，任务 7）。
    OpeningBalance,
    /// 取得借款（现金入，负债增）。
    LoanDisbursement,
    /// 归还借款本金。
    LoanRepayment,
    /// 现金收入（现销/服务收现）。
    CashRevenue,
    /// 赊销（应收增、收入增，不动现金）。
    CreditSale,
    /// 应收回款（现金增、应收减，不重复计收入）。
    ReceivableCollection,
    /// 现金费用。
    CashExpense,
    /// 计提利息（费用增、应付利息增，非现金）。
    InterestAccrual,
    /// 支付利息。
    InterestPayment,
    /// 计提税费（Fixture 税率，未付）。
    TaxAccrual,
    /// 支付税费。
    TaxPayment,
    /// 直线折旧摊销（非现金）。
    Depreciation,
}

/// 现金流类别（CAS 31 三分类 + 非现金标识）：`NonCash` 与现金科目行互斥。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum CashFlowClass {
    Operating,
    Investing,
    Financing,
    NonCash,
}

/// 借贷方向。行金额恒正，方向唯一地由这里表达。
#[derive(
    Copy,
    Clone,
    Eq,
    PartialEq,
    Ord,
    PartialOrd,
    Hash,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    ts_rs::TS,
)]
pub enum PostingSide {
    Debit,
    Credit,
}

/// 分录行：科目 + 方向 + 正金额。字段公开（凭证是值对象）；不变量在
/// [`JournalEntry::validate_invariants`] 与恢复重放的同一验证路径强制。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct JournalLine {
    pub account: super::ledger::LedgerAccountId,
    pub side: PostingSide,
    pub amount: AccountingAmount,
}

/// 一笔分录：来源事件 + 日期 + 业务种类 + 现金流类别 + 借贷行。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct JournalEntry {
    pub source: BusinessEventId,
    pub date: CivilDate,
    pub kind: BusinessKind,
    pub cash_flow: CashFlowClass,
    pub lines: Vec<JournalLine>,
}

impl JournalEntry {
    /// 该笔分录所属会计期间（由已验证日期折算，不可失败）。
    pub fn period(&self) -> AccountingPeriod {
        AccountingPeriod::of_date(self.date)
    }

    /// 复式不变量验证（过账与存档恢复共用同一路径）：
    /// 非空行、每行正金额、借贷两侧俱全、借贷总额相等。
    pub fn validate_invariants(&self) -> Result<(), AccountingError> {
        if self.lines.is_empty() {
            return Err(AccountingError::EmptyLines { event: self.source });
        }
        let mut has_debit = false;
        let mut has_credit = false;
        let mut debit_total = AccountingAmount::ZERO;
        let mut credit_total = AccountingAmount::ZERO;
        for line in &self.lines {
            if !line.amount.is_positive() {
                return Err(AccountingError::NonPositiveLine {
                    event: self.source,
                    account: line.account.clone(),
                    side: line.side,
                    amount: line.amount,
                });
            }
            match line.side {
                PostingSide::Debit => {
                    has_debit = true;
                    debit_total = debit_total.add(line.amount)?;
                }
                PostingSide::Credit => {
                    has_credit = true;
                    credit_total = credit_total.add(line.amount)?;
                }
            }
        }
        match (has_debit, has_credit) {
            (true, true) => {}
            (true, false) => {
                return Err(AccountingError::MissingSide {
                    event: self.source,
                    missing: "credit",
                })
            }
            (false, _) => {
                return Err(AccountingError::MissingSide {
                    event: self.source,
                    missing: "debit",
                })
            }
        }
        if debit_total != credit_total {
            return Err(AccountingError::Unbalanced {
                event: self.source,
                date: self.date,
                debit_total,
                credit_total,
            });
        }
        Ok(())
    }
}
