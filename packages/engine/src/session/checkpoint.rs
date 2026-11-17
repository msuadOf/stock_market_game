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

    pub fn classify_receipt(&self, key: CheckpointKey) -> ReceiptDisposition {
        if self
            .last_receipt
            .is_some_and(|last| last.seq == key.seq && last != key)
            && (key.generation, key.writer_epoch) == (self.generation, self.writer_epoch)
        {
            return ReceiptDisposition::OutOfOrder;
        }
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
        ReceiptDisposition::Accepted
    }

    pub fn accept_receipt(&mut self, key: CheckpointKey) -> ReceiptDisposition {
        let disposition = self.classify_receipt(key);
        if disposition == ReceiptDisposition::Accepted {
            let _ = self.pending.pop_front();
            self.last_receipt = Some(key);
        }
        disposition
    }
}

/// 归档结果属于权威状态，而不是观察者或传输状态。
#[derive(Debug, PartialEq, Eq)]
pub enum ReceiptOutcome {
    Durable,
    Stale,
    Failed { key: CheckpointKey, message: String },
}

#[cfg(any(test, feature = "verification-harness"))]
#[derive(Debug, PartialEq, Eq)]
pub struct ArchiveTrace {
    pub queued: Vec<CheckpointKey>,
    pub durable: Option<CheckpointKey>,
    pub blocked: bool,
}

/// 三端共用的捕获、交接和收据事务。失败后明确保持 blocked，保留原 sealed
/// candidate；不在缺少 attempt fencing 的通道上盲重试，也不重新执行业务日结。
/// 存储实际写入仍须验证 writer lease，内存收据不能代替数据库围栏。
pub struct ArchiveAuthority {
    order: CheckpointOrder,
    retained: VecDeque<CapturedCheckpoint>,
    blocked: Option<String>,
    blocked_at: Option<CheckpointKey>,
    durable: Option<CheckpointKey>,
}

impl ArchiveAuthority {
    pub fn new(generation: u64, writer_epoch: u64) -> Self {
        Self {
            order: CheckpointOrder::new(generation, writer_epoch),
            retained: VecDeque::new(),
            blocked: None,
            blocked_at: None,
            durable: None,
        }
    }

    pub fn install(&mut self, generation: u64, writer_epoch: u64) {
        if (generation, writer_epoch) != (self.order.generation, self.order.writer_epoch) {
            *self = Self::new(generation, writer_epoch);
        }
    }

    #[cfg(any(test, feature = "verification-harness"))]
    pub fn trace(&self) -> ArchiveTrace {
        ArchiveTrace {
            queued: self.retained.iter().map(|item| item.key).collect(),
            durable: self.durable,
            blocked: self.is_blocked(),
        }
    }
    pub fn is_blocked(&self) -> bool {
        self.blocked.is_some()
    }
    pub fn blocked_reason(&self) -> Option<&str> {
        self.blocked.as_deref()
    }
    pub fn durable(&self) -> Option<CheckpointKey> {
        self.durable
    }
    pub fn pending_count(&self) -> usize {
        self.retained.len()
    }
    pub fn latest_staged(&self) -> Option<CheckpointKey> {
        self.order.last_staged
    }
    pub fn pending_key(&self, seq: u64) -> Option<CheckpointKey> {
        self.retained
            .iter()
            .find(|item| item.key.seq == seq)
            .map(|item| item.key)
    }

    pub fn block(&mut self, reason: String) {
        if self.blocked.is_none() {
            self.blocked = Some(reason);
        }
    }

    fn block_at(&mut self, key: CheckpointKey, reason: String) {
        if self.blocked.is_none() {
            self.blocked_at = Some(key);
        }
        self.block(reason);
    }

    pub fn ensure_drained(&self) -> Result<(), String> {
        if let Some(reason) = &self.blocked {
            return Err(format!(
                "日终归档处于 blocked，{} 份候选尚未确认耐久：{reason}",
                self.retained.len()
            ));
        }
        if !self.retained.is_empty() {
            return Err(format!(
                "{} 份日终归档仅已排队，尚未确认耐久",
                self.retained.len()
            ));
        }
        Ok(())
    }

    pub fn stage(
        &mut self,
        session: &ProtocolSession,
        update: &CivilUpdate,
        enqueue: impl FnOnce(CapturedCheckpoint) -> Result<(), String>,
    ) -> Result<(), String> {
        if let Some(reason) = &self.blocked {
            return Err(reason.clone());
        }
        let captured = CapturedCheckpoint::take(
            session,
            update,
            self.order.generation,
            self.order.writer_epoch,
        )
        .map_err(|error| error.to_string());
        match captured {
            Ok(captured) => self.stage_captured(captured, enqueue),
            Err(error) => {
                self.block(error.clone());
                Err(error)
            }
        }
    }

    pub fn stage_captured(
        &mut self,
        captured: CapturedCheckpoint,
        enqueue: impl FnOnce(CapturedCheckpoint) -> Result<(), String>,
    ) -> Result<(), String> {
        if let Some(reason) = &self.blocked {
            return Err(reason.clone());
        }
        let key = captured.key;
        // 每日仅增加 Arc 引用；绝不复制 SaveSlot 正文，也不在 tick 上编码。
        self.retained.push_back(captured.clone());
        let result = self
            .order
            .can_stage(key)
            .map_err(|error| error.to_string())
            .and_then(|()| enqueue(captured))
            .and_then(|()| self.order.staged(key).map_err(|error| error.to_string()));
        if let Err(error) = &result {
            self.block_at(key, error.clone());
        }
        result
    }

    pub fn complete_sequence(
        &mut self,
        generation: u64,
        epoch: u64,
        seq: u64,
        result: Result<(), String>,
    ) -> Result<ReceiptOutcome, String> {
        if (generation, epoch) != (self.order.generation, self.order.writer_epoch) {
            return Ok(ReceiptOutcome::Stale);
        }
        if self.durable.is_some_and(|key| seq < key.seq) {
            return Ok(ReceiptOutcome::Stale);
        }
        let key = self
            .pending_key(seq)
            .or_else(|| self.durable.filter(|key| key.seq == seq));
        match key {
            Some(key) => Ok(self.complete(key, result)),
            None => {
                let message = format!("归档任务返回未知或非 FIFO 序号：{seq}");
                self.block(message.clone());
                Err(message)
            }
        }
    }

    pub fn complete(&mut self, key: CheckpointKey, result: Result<(), String>) -> ReceiptOutcome {
        match self.order.classify_receipt(key) {
            ReceiptDisposition::Stale => return ReceiptOutcome::Stale,
            ReceiptDisposition::OutOfOrder => {
                let message = "归档任务返回未知、损坏或非 FIFO 收据".to_owned();
                self.block(message.clone());
                return ReceiptOutcome::Failed { key, message };
            }
            ReceiptDisposition::Accepted => {}
        }
        if let Some(reason) = &self.blocked {
            if self.blocked_at.is_none_or(|blocked| key.seq >= blocked.seq) {
                return ReceiptOutcome::Failed {
                    key,
                    message: format!("归档仍被阻止，收据不能解除此前故障：{reason}"),
                };
            }
        }
        if let Err(message) = result {
            // 先前较晚的入队失败不遮盖更早写入失败的阻断位置。
            if self.blocked_at.is_some_and(|blocked| key.seq < blocked.seq) {
                self.blocked_at = Some(key);
            }
            self.block_at(key, message.clone());
            return ReceiptOutcome::Failed { key, message };
        }
        self.order.accept_receipt(key);
        let removed = self
            .retained
            .pop_front()
            .expect("已登记的归档必须保留 sealed candidate");
        assert_eq!(removed.key, key, "归档候选与收据队头不一致");
        self.durable = Some(key);
        ReceiptOutcome::Durable
    }
}

/// 存储侧使用同一写序列围栏：首错之后不写较晚候选，避免快速槽越过漏档。
/// 此状态只决定业务顺序；实际 writer lease 的原子检查仍由存储机制完成。
#[derive(Default)]
pub struct ArchiveWriteFence {
    binding: Option<(u64, u64)>,
    failure: Option<String>,
}

impl ArchiveWriteFence {
    pub fn begin(&mut self, key: CheckpointKey) -> Result<(), String> {
        let binding = (key.generation, key.writer_epoch);
        if self.binding.is_some_and(|current| binding < current) {
            return Err("旧 writer 任务不能进入新归档序列".into());
        }
        if self.binding != Some(binding) {
            self.binding = Some(binding);
            self.failure = None;
        }
        self.ensure_finished()
    }
    pub fn finish(&mut self, key: CheckpointKey, result: &Result<(), String>) {
        if self.binding == Some((key.generation, key.writer_epoch)) {
            if let Err(error) = result {
                if self.failure.is_none() {
                    self.failure = Some(error.clone());
                }
            }
        }
    }
    pub fn ensure_finished(&self) -> Result<(), String> {
        match &self.failure {
            Some(error) => Err(format!("归档写序列已 blocked，尚有未耐久候选：{error}")),
            None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests;
