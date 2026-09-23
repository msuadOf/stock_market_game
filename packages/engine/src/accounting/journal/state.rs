//! 权威日记账状态：批次（原子单位）、来源索引、封账状态与 serde（只存事实）。

use std::collections::BTreeMap;

use super::{BusinessEventId, JournalEntry};
use crate::accounting::error::AccountingError;
use crate::accounting::period::{AccountingPeriod, PeriodStates};
use crate::calendar::CivilDate;

/// 权威日记账。批次是原子单位：要么整批入账、要么零痕迹（验证在
/// [`crate::accounting::Books::post_batch`]，本结构只承载已提交事实）。
#[derive(Clone, Eq, PartialEq, Debug, Default)]
pub struct Journal {
    pub(crate) batches: Vec<Vec<JournalEntry>>,
    /// 来源 → 首次入账日期（派生自 batches，恢复时重建；不在存档中出现）。
    posted: BTreeMap<BusinessEventId, CivilDate>,
    pub(crate) periods: PeriodStates,
}

impl Journal {
    pub fn new() -> Self {
        Self::default()
    }

    /// 提交一批已通过全部验证的分录（仅 Books 验证通过后调用）。
    pub(crate) fn record_batch(&mut self, batch: Vec<JournalEntry>) {
        for entry in &batch {
            self.posted.insert(entry.source, entry.date);
        }
        self.batches.push(batch);
    }

    /// 该来源是否已入账（及首次日期）。
    pub fn posted_date(&self, event: BusinessEventId) -> Option<CivilDate> {
        self.posted.get(&event).copied()
    }

    /// 入账前检查：来源唯一 + 期间开放（批内查重由 Books 对同批先行集合处理）。
    pub(crate) fn check_postable(&self, entry: &JournalEntry) -> Result<(), AccountingError> {
        if let Some(first_date) = self.posted.get(&entry.source) {
            return Err(AccountingError::DuplicatePosting {
                event: entry.source,
                first_date: *first_date,
            });
        }
        let period = entry.period();
        if self.periods.is_closed(period) {
            return Err(AccountingError::ClosedPeriod {
                period,
                event: entry.source,
            });
        }
        Ok(())
    }

    /// 封账（状态 + 守卫；结账机制在任务 13）。
    pub fn close_period(&mut self, period: AccountingPeriod) -> Result<(), AccountingError> {
        self.periods.close(period)
    }

    pub fn period_status(&self, period: AccountingPeriod) -> crate::accounting::PeriodStatus {
        self.periods.status(period)
    }

    pub fn entry_count(&self) -> usize {
        self.batches.iter().map(Vec::len).sum()
    }

    /// 全部已入账分录（跨批次展平，入账顺序）。
    pub fn entries(&self) -> impl Iterator<Item = &JournalEntry> {
        self.batches.iter().flatten()
    }

    /// 消耗为批次序列（恢复重放用）。
    pub(crate) fn into_batches(self) -> Vec<Vec<JournalEntry>> {
        self.batches
    }

    /// 已封期间升序（恢复重放用）。
    pub(crate) fn closed_periods(&self) -> impl Iterator<Item = AccountingPeriod> + '_ {
        self.periods.closed_periods()
    }
}

/// serde 只保存**事实**：批次序列 + 已封期间。posted 由恢复重放重建；
/// 存档里出现重复来源会在重放时显式失败（见 Books 反序列化）。
#[derive(serde::Serialize)]
struct JournalSaveRef<'a> {
    batches: &'a [Vec<JournalEntry>],
    closed: Vec<AccountingPeriod>,
}

#[derive(serde::Deserialize)]
struct JournalSaveOwned {
    batches: Vec<Vec<JournalEntry>>,
    closed: Vec<AccountingPeriod>,
}

impl serde::Serialize for Journal {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        JournalSaveRef {
            batches: &self.batches,
            closed: self.periods.closed_periods().collect(),
        }
        .serialize(serializer)
    }
}

impl<'de> serde::Deserialize<'de> for Journal {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let save = JournalSaveOwned::deserialize(deserializer)?;
        let mut journal = Journal::new();
        for period in save.closed {
            journal
                .periods
                .close(period)
                .map_err(serde::de::Error::custom)?;
        }
        journal.batches = save.batches;
        for entry in journal.batches.iter().flatten() {
            if journal.posted.insert(entry.source, entry.date).is_some() {
                return Err(serde::de::Error::custom(format!(
                    "duplicate source {:?} in journal save",
                    entry.source
                )));
            }
        }
        Ok(journal)
    }
}
