//! 固定真实名册内的纯覆盖表；由 RuntimeDriver 接管已提交输入。
//! source_seq 只验证当前待汇合窗口，不是公共事件 seq，也不保存跨成功窗口历史。

use super::{GroupIdx, LocalIdx};
use std::collections::{BTreeMap, BTreeSet};

/// 稳定账户坐标，不含 source_seq。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SyncOwner {
    pub group: GroupIdx,
    pub local: LocalIdx,
}

/// 一次已提交输入的覆盖身份与最新整态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SyncValue<T> {
    pub source_seq: u64,
    pub value: T,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SyncDomain {
    Account,
    Belief,
    Attention,
}

/// 不定义具体业务提交版本；只保留待消费的 Full 原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum FullReason {
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "ExplicitRequest 是保留给明确全量同步请求的契约原因"
        )
    )]
    ExplicitRequest,
    DomainMutationWithoutFacts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AccountSource {
    Full,
    SparseAccumulated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecordOutcome {
    Changed,
    Unchanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlushOutcome {
    Successful,
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "Failed 表示失败 flush 必须保留待同步窗口的契约结果"
        )
    )]
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum SyncError {
    DuplicateGroup {
        group: GroupIdx,
    },
    RosterSizeOverflow,
    UnknownGroup {
        owner: SyncOwner,
    },
    LocalOutsideRoster {
        owner: SyncOwner,
        real_accounts: LocalIdx,
    },
    OlderSource {
        domain: SyncDomain,
        owner: SyncOwner,
        current: u64,
        incoming: u64,
    },
    ConflictingSource {
        domain: SyncDomain,
        owner: SyncOwner,
        source_seq: u64,
    },
}

impl std::fmt::Display for SyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "driver_sync 覆盖输入或真实名册不合法：{self:?}；请反馈 owner、domain 与 source_seq"
        )
    }
}
impl std::error::Error for SyncError {}

/// 三张表每张最多 A 个 owner；不持有 World、session 或事实历史。
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SyncAccumulator<A, B, T> {
    roster: BTreeMap<GroupIdx, LocalIdx>,
    real_account_count: usize,
    accounts: BTreeMap<SyncOwner, SyncValue<A>>,
    beliefs: BTreeMap<SyncOwner, SyncValue<B>>,
    attentions: BTreeMap<SyncOwner, SyncValue<T>>,
    plans_pending: bool,
    pending_full_reasons: BTreeSet<FullReason>,
    has_successful_baseline: bool,
}

impl<A: PartialEq, B: PartialEq, T: PartialEq> SyncAccumulator<A, B, T> {
    /// 每组长度只含真实账户，不得传入 worker 容量或尾组 padding。
    pub(crate) fn new(
        roster: impl IntoIterator<Item = (GroupIdx, LocalIdx)>,
    ) -> Result<Self, SyncError> {
        let mut groups = BTreeMap::new();
        let mut total = 0usize;
        for (group, real_accounts) in roster {
            if groups.insert(group, real_accounts).is_some() {
                return Err(SyncError::DuplicateGroup { group });
            }
            total = total
                .checked_add(real_accounts as usize)
                .ok_or(SyncError::RosterSizeOverflow)?;
        }
        Ok(Self {
            roster: groups,
            real_account_count: total,
            accounts: BTreeMap::new(),
            beliefs: BTreeMap::new(),
            attentions: BTreeMap::new(),
            plans_pending: false,
            pending_full_reasons: BTreeSet::new(),
            has_successful_baseline: false,
        })
    }
    pub(crate) fn record_account(
        &mut self,
        owner: SyncOwner,
        source_seq: u64,
        value: A,
    ) -> Result<RecordOutcome, SyncError> {
        record(
            &self.roster,
            &mut self.accounts,
            SyncDomain::Account,
            owner,
            source_seq,
            value,
        )
    }
    pub(crate) fn record_belief(
        &mut self,
        owner: SyncOwner,
        source_seq: u64,
        value: B,
    ) -> Result<RecordOutcome, SyncError> {
        record(
            &self.roster,
            &mut self.beliefs,
            SyncDomain::Belief,
            owner,
            source_seq,
            value,
        )
    }
    pub(crate) fn record_attention(
        &mut self,
        owner: SyncOwner,
        source_seq: u64,
        value: T,
    ) -> Result<RecordOutcome, SyncError> {
        record(
            &self.roster,
            &mut self.attentions,
            SyncDomain::Attention,
            owner,
            source_seq,
            value,
        )
    }
    pub(crate) fn accounts(&self) -> &BTreeMap<SyncOwner, SyncValue<A>> {
        &self.accounts
    }
    pub(crate) fn beliefs(&self) -> &BTreeMap<SyncOwner, SyncValue<B>> {
        &self.beliefs
    }
    pub(crate) fn attentions(&self) -> &BTreeMap<SyncOwner, SyncValue<T>> {
        &self.attentions
    }
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "真实账户总数只由覆盖表单元测试读取；字段仍由此 getter 消费"
        )
    )]
    pub(crate) fn real_account_count(&self) -> usize {
        self.real_account_count
    }
    pub(crate) fn mark_plans_pending(&mut self) {
        self.plans_pending = true;
    }
    pub(crate) fn plans_pending(&self) -> bool {
        self.plans_pending
    }
    pub(crate) fn require_full(&mut self, reason: FullReason) {
        self.pending_full_reasons.insert(reason);
    }
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "待处理 Full 原因的只读视图只由覆盖表单元测试读取")
    )]
    pub(crate) fn pending_full_reasons(&self) -> &BTreeSet<FullReason> {
        &self.pending_full_reasons
    }
    pub(crate) fn has_successful_baseline(&self) -> bool {
        self.has_successful_baseline
    }
    pub(crate) fn account_source(&self) -> AccountSource {
        if !self.has_successful_baseline || !self.pending_full_reasons.is_empty() {
            AccountSource::Full
        } else {
            AccountSource::SparseAccumulated
        }
    }
    /// 仅报告外部安装结果；不执行安装、重试、回滚或 terminal 策略。
    pub(crate) fn complete_flush(&mut self, outcome: FlushOutcome) {
        if outcome == FlushOutcome::Successful {
            self.accounts.clear();
            self.beliefs.clear();
            self.attentions.clear();
            self.plans_pending = false;
            self.pending_full_reasons.clear();
            self.has_successful_baseline = true;
        }
    }
}

/// 所有校验均在写入之前；失败时三张表、plans 和 Full 状态都不改变。
fn record<T: PartialEq>(
    roster: &BTreeMap<GroupIdx, LocalIdx>,
    table: &mut BTreeMap<SyncOwner, SyncValue<T>>,
    domain: SyncDomain,
    owner: SyncOwner,
    source_seq: u64,
    value: T,
) -> Result<RecordOutcome, SyncError> {
    let real_accounts = roster
        .get(&owner.group)
        .ok_or(SyncError::UnknownGroup { owner })?;
    if owner.local >= *real_accounts {
        return Err(SyncError::LocalOutsideRoster {
            owner,
            real_accounts: *real_accounts,
        });
    }
    if let Some(current) = table.get(&owner) {
        if source_seq < current.source_seq {
            return Err(SyncError::OlderSource {
                domain,
                owner,
                current: current.source_seq,
                incoming: source_seq,
            });
        }
        if source_seq == current.source_seq {
            return if current.value == value {
                Ok(RecordOutcome::Unchanged)
            } else {
                Err(SyncError::ConflictingSource {
                    domain,
                    owner,
                    source_seq,
                })
            };
        }
    }
    table.insert(owner, SyncValue { source_seq, value });
    Ok(RecordOutcome::Changed)
}

#[cfg(test)]
#[path = "driver_sync_tests.rs"]
mod tests;
