//! 权威线程到独立归档 Worker 的非阻塞交接；不在推进线程编码存档。
use super::{error, to_js};
use engine::session::{
    checkpoint::{
        CapturedCheckpoint, CheckpointOrder, ReceiptDisposition, MAX_PENDING_CHECKPOINTS,
    },
    protocol::{CivilUpdate, ProtocolSession},
};
use libos::{
    observer_mailbox::{self, Receiver, Sender, TryRecvError},
    wake_epoch::WakeEpoch,
};
use serde::Serialize;
use std::{
    cell::Cell,
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
use wasm_bindgen::prelude::*;

struct Consumer {
    rx: Receiver<CapturedCheckpoint>,
    wake: Arc<WakeEpoch>,
}
static CONSUMER: Mutex<Option<Consumer>> = Mutex::new(None);

pub(super) struct ArchivePort {
    tx: Sender<CapturedCheckpoint>,
    wake: Arc<WakeEpoch>,
    pub generation: u64,
    pub writer_epoch: u64,
    staged: Cell<Option<u64>>,
    order: CheckpointOrder,
}

impl ArchivePort {
    pub fn open(generation: u64, writer_epoch: u64) -> Result<Self, JsValue> {
        let mut consumer = CONSUMER.lock().map_err(|_| error("归档邮箱锁损坏"))?;
        if consumer.is_some() {
            return Err(error("旧归档任务尚未释放"));
        }
        let (tx, rx) = observer_mailbox::channel(
            NonZeroUsize::new(MAX_PENDING_CHECKPOINTS).expect("归档队列容量必须为正"),
        );
        let wake = Arc::new(WakeEpoch::new());
        *consumer = Some(Consumer {
            rx,
            wake: Arc::clone(&wake),
        });
        Ok(Self {
            tx,
            wake,
            generation,
            writer_epoch,
            staged: Cell::new(None),
            order: CheckpointOrder::new(generation, writer_epoch),
        })
    }

    pub fn latest_staged(&self) -> Option<u64> {
        self.staged.get()
    }

    pub fn receive(&mut self, sequence: u64) -> Result<(), JsValue> {
        match self
            .order
            .accept_pending_seq(self.generation, self.writer_epoch, sequence)
        {
            ReceiptDisposition::Accepted | ReceiptDisposition::Stale => Ok(()),
            ReceiptDisposition::OutOfOrder => Err(error("归档任务返回非 FIFO 收据")),
        }
    }

    pub fn try_stage(
        &mut self,
        session: &ProtocolSession,
        update: &CivilUpdate,
    ) -> Result<(), String> {
        let value = CapturedCheckpoint::take(session, update, self.generation, self.writer_epoch)
            .map_err(|error| error.to_string())?;
        let key = value.key;
        self.order
            .can_stage(key)
            .map_err(|error| error.to_string())?;
        let result = self.tx.try_send(value);
        match result {
            Ok(()) => {
                self.order.staged(key).map_err(|error| error.to_string())?;
                self.staged.set(Some(key.seq));
                self.wake.notify();
                Ok(())
            }
            Err(error) => {
                let (description, rejected) = match error {
                    observer_mailbox::TrySendError::Full(value) => {
                        ("日终归档队列已满，当前候选未写入", value)
                    }
                    observer_mailbox::TrySendError::Disconnected(value) => {
                        ("日终归档任务已退出，当前候选未写入", value)
                    }
                };
                // 最新归档仍由会话保有；落后任务失败不能冒充已耐久。
                libos::deferred_drop::defer(rejected);
                Err(description.into())
            }
        }
    }
}

impl Drop for ArchivePort {
    fn drop(&mut self) {
        self.wake.close();
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum WireCheckpoint<'a> {
    Closed,
    #[serde(rename_all = "camelCase")]
    Checkpoint {
        generation: String,
        writer_epoch: String,
        seq: String,
        slot: &'a engine::SaveSlot,
    },
}

#[wasm_bindgen]
pub fn market_archive_signal() -> Result<u32, JsValue> {
    let consumer = CONSUMER.lock().map_err(|_| error("归档邮箱锁损坏"))?;
    consumer
        .as_ref()
        .and_then(|value| value.wake.shared_memory_offset())
        .ok_or_else(|| error("归档邮箱缺少信号"))
}

#[wasm_bindgen]
pub fn market_archive_take() -> Result<JsValue, JsValue> {
    let value = {
        let consumer = CONSUMER.lock().map_err(|_| error("归档邮箱锁损坏"))?;
        let consumer = consumer.as_ref().ok_or_else(|| error("归档邮箱尚未安装"))?;
        match consumer.rx.try_recv() {
            Ok(value) => value,
            Err(TryRecvError::Empty) if !consumer.wake.is_closed() => return Ok(JsValue::NULL),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                return to_js(&WireCheckpoint::Closed)
            }
        }
    };
    // 只在归档 Worker 上克隆到 JS；锁不跨编码或持久化。
    to_js(&WireCheckpoint::Checkpoint {
        generation: value.key.generation.to_string(),
        writer_epoch: value.key.writer_epoch.to_string(),
        seq: value.key.seq.to_string(),
        slot: &value.slot,
    })
}

#[wasm_bindgen]
pub fn market_archive_release() -> Result<(), JsValue> {
    let mut consumer = CONSUMER.lock().map_err(|_| error("归档邮箱锁损坏"))?;
    if consumer.take().is_none() {
        return Err(error("归档邮箱已释放"));
    }
    Ok(())
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn market_archive_tombstone() -> Result<u32, JsValue> {
    libos::thread_facade::auxiliary_tombstone().map_err(error)
}

#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn market_archive_bind_tombstone(address: u32) -> Result<(), JsValue> {
    libos::thread_facade::bind_auxiliary_tombstone(address).map_err(error)
}
