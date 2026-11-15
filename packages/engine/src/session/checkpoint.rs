//! 已提交日终快照的非阻塞交接与顺序合同，不拥有任何 I/O 实现。

use super::protocol::{CivilUpdate, ProtocolSession, SaveCandidateKey};
use super::{SessionError, StepFatal};
use crate::{CivilDate, SaveSlot};
use std::collections::VecDeque;
use std::sync::Arc;

/// 权威侧允许尚未确认的已排队日终数量。观察者 ACK 不参与此计数。
pub const MAX_PENDING_CHECKPOINTS: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CheckpointKey {
    pub generation: u64,
    pub writer_epoch: u64,
    pub tick: u64,
    pub seq: u64,
    pub settled_date: CivilDate,
    pub day: u32,
}

#[derive(Clone, Debug)]
pub struct CapturedCheckpoint {
    pub key: CheckpointKey,
    pub slot: Arc<SaveSlot>,
}

impl CapturedCheckpoint {
    /// 在提交同一个日界的权威轮中移动 Arc，不复制存档内容，也不等待写盘。
    pub fn take(
        session: &ProtocolSession,
        update: &CivilUpdate,
        generation: u64,
        writer_epoch: u64,
    ) -> Result<Self, SessionError> {
        let key = CheckpointKey {
            generation,
            writer_epoch,
            tick: update.tick,
            seq: update.seq_to,
            settled_date: update.boundary.settled_date,
            day: session.game().day(),
        };
        let exact_latest = session
            .latest_day_end_key()
            .is_some_and(|latest| latest.seq == key.seq && latest.settled_date == key.settled_date);
        if session.tick() != key.tick || !exact_latest {
            return Err(StepFatal::InvariantViolation {
                location: "CapturedCheckpoint::take".into(),
                description: "日终候选不对应刚提交的权威日界".into(),
            }
            .into());
        }
        let slot = session.take_unstaged_save_candidate(&SaveCandidateKey {
            seq: key.seq,
            settled_date: key.settled_date,
        })?;
        Ok(Self { key, slot })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CheckpointOrderError {
    #[error("当前存档任务不属于活动会话或 writer 租约")]
    WrongBinding,
    #[error("日终存档候选未按自然日和事实序号推进")]
    NonIncreasing,
    #[error("日终存档队列已满，尚未写入当前候选")]
    Full,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiptDisposition {
    Accepted,
    Stale,
    OutOfOrder,
}

/// 权威侧的有限 FIFO 收据窗口。存储实现仍须在实际写入侧检查租约，
/// 单靠观察此收据不能阻止早先的任务覆盖新档。
pub struct CheckpointOrder {
    generation: u64,
    writer_epoch: u64,
    pending: VecDeque<CheckpointKey>,
    last_staged: Option<CheckpointKey>,
    last_receipt: Option<CheckpointKey>,
}

impl CheckpointOrder {
    pub fn new(generation: u64, writer_epoch: u64) -> Self {
        Self {
            generation,
            writer_epoch,
            pending: VecDeque::new(),
            last_staged: None,
            last_receipt: None,
        }
    }

    pub fn install(&mut self, generation: u64, writer_epoch: u64) {
        if (generation, writer_epoch) != (self.generation, self.writer_epoch) {
            *self = Self::new(generation, writer_epoch);
        }
    }

    pub fn can_stage(&self, key: CheckpointKey) -> Result<(), CheckpointOrderError> {
        if (key.generation, key.writer_epoch) != (self.generation, self.writer_epoch) {
            return Err(CheckpointOrderError::WrongBinding);
        }
        if self.last_staged.is_some_and(|last| {
            key.seq <= last.seq || key.settled_date <= last.settled_date || key.tick < last.tick
        }) {
            return Err(CheckpointOrderError::NonIncreasing);
        }
        if self.pending.len() >= MAX_PENDING_CHECKPOINTS {
            return Err(CheckpointOrderError::Full);
        }
        Ok(())
    }

    /// 宿主仅在 transport 已接手任务后登记；槽满或关闭不会留下假收据。
    pub fn staged(&mut self, key: CheckpointKey) -> Result<(), CheckpointOrderError> {
        self.can_stage(key)?;
        self.last_staged = Some(key);
        self.pending.push_back(key);
        Ok(())
    }

    /// 专用 Worker 的串行端口只回传轻量序号。本地用确切队头还原完整围栏，
    /// 不允许较新的序号跳过尚未确认的候选。
    pub fn accept_pending_seq(
        &mut self,
        generation: u64,
        writer_epoch: u64,
        seq: u64,
    ) -> ReceiptDisposition {
        if (generation, writer_epoch) != (self.generation, self.writer_epoch)
            || self
                .last_receipt
                .is_some_and(|previous| seq <= previous.seq)
        {
            return ReceiptDisposition::Stale;
        }
        match self.pending.front().copied() {
            Some(key) if key.seq == seq => self.accept_receipt(key),
            _ => ReceiptDisposition::OutOfOrder,
        }
    }

    pub fn accept_receipt(&mut self, key: CheckpointKey) -> ReceiptDisposition {
        if (key.generation, key.writer_epoch) != (self.generation, self.writer_epoch)
            || self
                .last_receipt
                .is_some_and(|previous| key.seq <= previous.seq)
        {
            return ReceiptDisposition::Stale;
        }
        if self.pending.front() != Some(&key) {
            return ReceiptDisposition::OutOfOrder;
        }
        let _ = self.pending.pop_front();
        self.last_receipt = Some(key);
        ReceiptDisposition::Accepted
    }
}

#[cfg(test)]
mod tests;
