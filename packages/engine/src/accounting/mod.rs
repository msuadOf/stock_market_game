//! 原子复式记账与总账底座（K2，任务 6）。
//!
//! 分层：`amount`（checked i128 分 + 基点/余数/十进制字符串 serde）→
//! `period`（会计期间 + 封账状态与守卫）→ `journal`（权威日记账：来源唯一、
//! 批次原子）→ `ledger`（科目表 + T 型余额与期间/现金流增量索引，派生投影）。
//! `Books` 组合两者并提供唯一过账入口：**整批先验证后原子提交**——逐笔分录
//! 错误不留半条账。
//!
//! 事实/派生边界（K2）：Journal 是权威事实并随存档保存；Ledger 索引只在
//! 内存中增量维护，恢复时重放重建（见 `Books` 反序列化）。行业科目表、
//! 结账机制与报表属任务 8–13；本模块保持通用与版本化。
//!
//! 铁律：无 f64；比例用整数基点；利息小数保留合同累计余数；落分统一整数
//! 半偶舍入；负权益/亏损是合法状态，负现金由过账守卫类型化拒绝（不 clamp）。

mod amount;
pub mod consolidation;
mod error;
mod fixed_assets;
mod inventory;
mod journal;
mod ledger;
mod period;
mod receivables;
pub mod reports;
mod tax;

pub use amount::{AccountingAmount, FractionUnits};
pub use consolidation::{
    consolidate, ConsolidatedBalance, ConsolidationError, ConsolidationOutput,
    ConsolidationRequest, DeclaredSide, GroupMember, IntercompanyBalance, IntercompanySale,
    MemberId, MemberSpec, MinorityInterest, ScopeId, SubsidiaryOwnership, WorksheetEntry,
    WorksheetLine, WorksheetReason,
};
pub use error::AccountingError;
pub use fixed_assets::{FixedAssetCode, FixedAssetEntry, FixedAssetError, FixedAssetRegister};
pub use inventory::{InventoryError, InventoryItemCode, InventoryItemState, InventoryLedger};
pub use journal::{
    BusinessEventId, BusinessKind, CashFlowClass, Journal, JournalEntry, JournalLine, PostingSide,
};
pub use ledger::{
    AccountBalance, AccountChart, AccountDef, AccountElement, Ledger, LedgerAccountId,
    TrialBalanceSummary,
};
pub use period::{AccountingPeriod, PeriodStates, PeriodStatus};
pub use receivables::{
    ecl_allowance_target, OpenItem, OpenItemId, TradeLedgerError, TradeOpenLedger,
};
pub use tax::{
    compute_income_tax, output_vat_on, split_input_vat, IncomeTaxComputation, IncomeTaxPolicy,
    InputVatSplit, LossEntry, TaxPolicy, TaxPolicyError, VatPolicy,
};

use std::collections::HashMap;

/// 公司账套 = 权威日记账 + 派生总账。投资者 `Account` 与公司经营账套
/// 完全隔离（K2 资金边界）；公司实体与开局账套在任务 7 落地。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Books {
    journal: Journal,
    ledger: Ledger,
}

impl Books {
    pub fn new(chart: AccountChart) -> Self {
        Self {
            journal: Journal::new(),
            ledger: Ledger::new(chart),
        }
    }

    /// 唯一过账入口：整批先验证后原子提交。
    ///
    /// 验证（任一失败 ⇒ 整批拒绝、账套零改动）：
    /// 1. 每笔分录复式不变量（非空行、正金额、借贷俱全、借贷总额相等）；
    /// 2. 来源唯一（历史已入账 / 批内重复 → `DuplicatePosting`）；
    /// 3. 期间开放（`ClosedPeriod`）；
    /// 4. 试算应用：科目在表、NonCash 不触现金、i128 合计不溢出；
    /// 5. 批级现金下限：批末每个现金科目余额 ≥ 0（`NegativeCashProhibited`）。
    ///
    /// 批次是原子单位：批内中间态不暴露（先出后进、批末非负即合法）。
    /// 空批为合法 no-op（不产生空批次记录）。
    pub fn post_batch(&mut self, batch: Vec<JournalEntry>) -> Result<(), AccountingError> {
        if batch.is_empty() {
            return Ok(());
        }
        let batch_len = batch.len();
        let abort =
            |failed_index: Option<usize>, cause: AccountingError| AccountingError::BatchAborted {
                batch_len,
                failed_index,
                cause: Box::new(cause),
            };

        let mut seen_in_batch = HashMap::new();
        for (idx, entry) in batch.iter().enumerate() {
            let structural = entry.validate_invariants().and_then(|()| {
                if let Some(first_date) = seen_in_batch.insert(entry.source, entry.date) {
                    return Err(AccountingError::DuplicatePosting {
                        event: entry.source,
                        first_date,
                    });
                }
                self.journal.check_postable(entry)
            });
            if let Err(cause) = structural {
                return Err(abort(Some(idx), cause));
            }
        }

        let mut trial = self.ledger.clone();
        for (idx, entry) in batch.iter().enumerate() {
            if let Err(cause) = trial.apply_entry(entry) {
                return Err(abort(Some(idx), cause));
            }
        }
        if let Err(cause) = check_cash_floor(&trial) {
            return Err(abort(None, cause));
        }

        self.ledger = trial;
        self.journal.record_batch(batch);
        Ok(())
    }

    /// 封账：状态 + 守卫（结账试算/结转/快照在任务 13）。
    pub fn close_period(&mut self, period: AccountingPeriod) -> Result<(), AccountingError> {
        self.journal.close_period(period)
    }

    /// 权威日记账（事实）。
    pub fn journal(&self) -> &Journal {
        &self.journal
    }

    /// 派生总账（由已提交分录增量构建的读投影）。
    pub fn ledger(&self) -> &Ledger {
        &self.ledger
    }
}

/// 批末现金下限：任一现金科目净借方余额 < 0 → 类型化拒绝（不 clamp）。
fn check_cash_floor(ledger: &Ledger) -> Result<(), AccountingError> {
    let cash_accounts: Vec<LedgerAccountId> = ledger
        .chart()
        .iter()
        .filter(|(_, def)| def.is_cash)
        .map(|(id, _)| id.clone())
        .collect();
    for account in cash_accounts {
        let projected = ledger.account_net_debit(&account)?;
        if projected.is_negative() {
            return Err(AccountingError::NegativeCashProhibited { account, projected });
        }
    }
    Ok(())
}

/// serde 只保存事实（科目表 + 日记账）；总账由恢复重放重建。
#[derive(serde::Serialize)]
struct BooksRef<'a> {
    chart: &'a AccountChart,
    journal: &'a Journal,
}

#[derive(serde::Deserialize)]
struct BooksOwned {
    chart: AccountChart,
    journal: Journal,
}

impl serde::Serialize for Books {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        BooksRef {
            chart: self.ledger.chart(),
            journal: &self.journal,
        }
        .serialize(serializer)
    }
}

/// 恢复 = 重放：逐批走与在线过账**同一条**验证路径（篡改存档注入非法/重复
/// 分录在此显式失败），再按期间序重放封账。任一步失败 → 整个恢复失败。
impl<'de> serde::Deserialize<'de> for Books {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let save = BooksOwned::deserialize(deserializer)?;
        let closed: Vec<AccountingPeriod> = save.journal.closed_periods().collect();
        let mut books = Books::new(save.chart);
        for batch in save.journal.into_batches() {
            books.post_batch(batch).map_err(serde::de::Error::custom)?;
        }
        for period in closed {
            books
                .close_period(period)
                .map_err(serde::de::Error::custom)?;
        }
        Ok(books)
    }
}
