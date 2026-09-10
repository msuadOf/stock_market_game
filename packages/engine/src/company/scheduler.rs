//! 经营到期事件队列（K4，任务 14）：持久化、稳定排序、替代整账扫描。
//!
//! 排序键 `(due_date, id)`——id 由队列单调分配，同日业务按提交顺序恰好一次
//! 派发。业务去重以调用方提供的**稳定来源 key** 承担（任务 5 时钟侧注释
//! 约定：同类业务可在同一日期注册多条）。股东动作（分红/回购/清算分配）是
//! K3 红线：结算仅设计，无运行时队列——送入即类型化拒绝。
//!
//! 时钟集成经会话接缝（`session::company_operations`）：调度器待办镜像为
//! `CivilClock` 的 `DueKind::InterestAccrual/ContractMaturity`，恰好一次
//! 语义与跳日守卫由时钟承担；本队列承载业务载荷（公司 + 引用）。

use std::collections::BTreeSet;

use crate::calendar::CivilDate;
use crate::company::spec::CompanyId;
use thiserror::Error;

/// 调度事件 id（单调分配，不复用）。
#[derive(
    Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug, serde::Serialize, serde::Deserialize,
)]
#[serde(transparent)]
pub struct ScheduledDueId(u64);

impl ScheduledDueId {
    pub fn value(self) -> u64 {
        self.0
    }
}

/// 到期业务动作（业务载荷；时钟侧只看到 [`DueKind`] 映射）。
///
/// [`DueKind`]: crate::session::DueKind
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum ScheduledAction {
    /// 滚动计提利息至到期日（ACT/365F 自然日）。
    InterestAccrual { company: CompanyId },
    /// 合同到期收付/回调。`reference` 语法：`AR:<应收开项>`（工商回款）、
    /// `LN:<贷款合同>`（银行收本收息）、`DL:<预售合同>`（地产交付）。
    /// 未知前缀 → 派发时类型化拒绝。
    ContractMaturity {
        company: CompanyId,
        reference: String,
    },
}

/// 提交请求。`ShareholderDistribution` 变体只存在于输入面——提交即拒绝，
/// 永不进入队列（K3：不执行任何股东分配）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub enum SchedulerRequest {
    Due {
        /// 稳定业务来源 key（如 `INT:C-1:2030-01-07`、`MAT:C-1:AR-9`）；
        /// 同 key 仍待办时重复提交被拒，弹出后可复用。
        key: String,
        due_date: CivilDate,
        action: ScheduledAction,
    },
    /// 股东分配动作（分红/回购/清算分配）——明确不支持。
    ShareholderDistribution { company: CompanyId, detail: String },
}

/// 一条待办到期事件。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ScheduledDue {
    pub id: ScheduledDueId,
    pub key: String,
    pub due_date: CivilDate,
    pub action: ScheduledAction,
}

/// 调度器错误（类型化；任何变体返回时队列零改动）。
#[derive(Clone, Eq, PartialEq, Debug, Error)]
pub enum SchedulerError {
    #[error("due key {key:?} is already pending (duplicate submission)")]
    DuplicateDueKey { key: String },
    #[error(
        "shareholder action for {company:?} ({detail}) is unsupported: settlements are design-only (K3)"
    )]
    ShareholderActionsUnsupported { company: CompanyId, detail: String },
    #[error("due {key:?} on {due_date} is not after the settled floor {floor:?}")]
    DueDateInPast {
        key: String,
        due_date: CivilDate,
        floor: CivilDate,
    },
    #[error("cannot dispatch due on {on}: earliest pending {earliest:?} would be skipped")]
    DueSkipped { on: CivilDate, earliest: CivilDate },
    #[error("scheduler state inconsistent: {detail}")]
    SaveInconsistent { detail: String },
}

/// 持久化到期队列。serde 保存 `{next_seq, settled_through, pending}`；恢复走
/// [`OperatingScheduler::from_parts`] 的全量校验（排序/唯一/单调——篡改存档
/// 在恢复边界显式失败）。
#[derive(Clone, Eq, PartialEq, Debug, Default, serde::Serialize)]
pub struct OperatingScheduler {
    next_seq: u64,
    /// 最后已派发日期；提交要求 due_date 严格晚于该日。
    settled_through: Option<CivilDate>,
    /// 按 (due_date, id) 升序。
    pending: Vec<ScheduledDue>,
}

impl OperatingScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn pending(&self) -> &[ScheduledDue] {
        &self.pending
    }

    pub fn pending_len(&self) -> usize {
        self.pending.len()
    }

    pub fn settled_through(&self) -> Option<CivilDate> {
        self.settled_through
    }

    /// 提交（先验证后落地）：股东动作一律拒绝；due 不得早于等于已结算日；
    /// 同 key 仍待办 → 重复拒绝。
    pub fn submit(&mut self, request: SchedulerRequest) -> Result<ScheduledDueId, SchedulerError> {
        let (key, due_date, action) = match request {
            SchedulerRequest::Due {
                key,
                due_date,
                action,
            } => (key, due_date, action),
            SchedulerRequest::ShareholderDistribution { company, detail } => {
                return Err(SchedulerError::ShareholderActionsUnsupported { company, detail });
            }
        };
        if let Some(floor) = self.settled_through {
            if due_date <= floor {
                return Err(SchedulerError::DueDateInPast {
                    key,
                    due_date,
                    floor,
                });
            }
        }
        if self.pending.iter().any(|due| due.key == key) {
            return Err(SchedulerError::DuplicateDueKey { key });
        }
        let id = ScheduledDueId(self.next_seq);
        self.next_seq += 1;
        let due = ScheduledDue {
            id,
            key,
            due_date,
            action,
        };
        let position = self
            .pending
            .partition_point(|existing| (existing.due_date, existing.id) < (due.due_date, id));
        self.pending.insert(position, due);
        Ok(id)
    }

    /// 派发 `on` 当日到期（按 (due_date, id) 稳定序）。存在更早的待办 →
    /// 跳日类型化拒绝（不静默丢失）；成功时推进 settled_through。
    pub fn pop_due_on(&mut self, on: CivilDate) -> Result<Vec<ScheduledDue>, SchedulerError> {
        if let Some(earliest) = self.pending.first() {
            if earliest.due_date < on {
                return Err(SchedulerError::DueSkipped {
                    on,
                    earliest: earliest.due_date,
                });
            }
        }
        let split = self.pending.partition_point(|due| due.due_date <= on);
        let dispatched = self.pending.drain(..split).collect();
        self.settled_through = Some(on);
        Ok(dispatched)
    }

    /// 恢复校验：排序、key 唯一、id 单调、无早于已结算日的待办。
    pub fn from_parts(
        next_seq: u64,
        settled_through: Option<CivilDate>,
        pending: Vec<ScheduledDue>,
    ) -> Result<Self, SchedulerError> {
        let inconsistent = |detail: String| SchedulerError::SaveInconsistent { detail };
        let mut keys = BTreeSet::new();
        for window in pending.windows(2) {
            if (window[0].due_date, window[0].id) >= (window[1].due_date, window[1].id) {
                return Err(inconsistent(format!(
                    "pending not sorted at {:?}",
                    window[0].key
                )));
            }
        }
        for due in &pending {
            if due.id.value() >= next_seq {
                return Err(inconsistent(format!(
                    "due id {} >= next_seq {next_seq}",
                    due.id.value()
                )));
            }
            if !keys.insert(due.key.clone()) {
                return Err(inconsistent(format!("duplicate key {:?}", due.key)));
            }
            if let Some(floor) = settled_through {
                if due.due_date <= floor {
                    return Err(inconsistent(format!(
                        "due {:?} on {} not after floor {floor}",
                        due.key, due.due_date
                    )));
                }
            }
        }
        Ok(Self {
            next_seq,
            settled_through,
            pending,
        })
    }
}

/// 恢复走 [`OperatingScheduler::from_parts`] 的全量校验（篡改存档在反序列化
/// 边界显式失败，与会计底座同一恢复纪律）。
impl<'de> serde::Deserialize<'de> for OperatingScheduler {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(serde::Deserialize)]
        struct Raw {
            next_seq: u64,
            settled_through: Option<CivilDate>,
            pending: Vec<ScheduledDue>,
        }
        let raw = Raw::deserialize(deserializer)?;
        OperatingScheduler::from_parts(raw.next_seq, raw.settled_through, raw.pending)
            .map_err(serde::de::Error::custom)
    }
}
