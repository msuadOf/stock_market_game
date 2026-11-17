//! 权威线程到独立归档 Worker 的非阻塞交接；不在推进线程编码存档。
use super::{error, to_js};
use engine::session::{
    checkpoint::{
        ArchiveAuthority, ArchiveWriteFence, CapturedCheckpoint, CheckpointKey, ReceiptOutcome,
        MAX_PENDING_CHECKPOINTS,
    },
    protocol::{CivilUpdate, ProtocolSession},
};
use libos::{
    observer_mailbox::{self, Receiver, Sender, TryRecvError},
    wake_epoch::WakeEpoch,
};
use serde::Serialize;
use std::{
    num::NonZeroUsize,
    sync::{Arc, Mutex},
};
use wasm_bindgen::prelude::*;

struct Consumer {
    rx: Receiver<CapturedCheckpoint>,
    fence: ArchiveWriteFence,
    writing: Option<CheckpointKey>,
    wake: Arc<WakeEpoch>,
}
static CONSUMER: Mutex<Option<Consumer>> = Mutex::new(None);
pub(super) struct ArchivePort {
    tx: Sender<CapturedCheckpoint>,
    wake: Arc<WakeEpoch>,
    pub generation: u64,
    pub writer_epoch: u64,
    order: ArchiveAuthority,
    #[cfg(feature = "verification-harness")]
    reject_next: bool,
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
            fence: ArchiveWriteFence::default(),
            writing: None,
            wake: Arc::clone(&wake),
        });
        Ok(Self {
            tx,
            wake,
            generation,
            writer_epoch,
            order: ArchiveAuthority::new(generation, writer_epoch),
            #[cfg(feature = "verification-harness")]
            reject_next: false,
        })
    }

    #[cfg(feature = "verification-harness")]
    pub fn reject_next_enqueue(&mut self) {
        self.reject_next = true;
    }

    pub fn latest_staged(&self) -> Option<u64> {
        self.order.latest_staged().map(|key| key.seq)
    }
    #[cfg(feature = "verification-harness")]
    pub fn trace(&self) -> engine::session::checkpoint::ArchiveTrace {
        self.order.trace()
    }
    pub fn is_blocked(&self) -> bool {
        self.order.is_blocked()
    }
    pub fn blocked_reason(&self) -> Option<&str> {
        self.order.blocked_reason()
    }
    pub fn block(&mut self, reason: String) {
        self.order.block(reason);
    }
    pub fn close(&self) {
        self.wake.close();
    }
    pub fn ensure_drained(&self) -> Result<(), String> {
        self.order.ensure_drained()
    }

    pub fn receive(
        &mut self,
        sequence: u64,
        result: Result<(), String>,
    ) -> Result<ReceiptOutcome, String> {
        self.order
            .complete_sequence(self.generation, self.writer_epoch, sequence, result)
    }

    pub fn try_stage(
        &mut self,
        session: &ProtocolSession,
        update: &CivilUpdate,
    ) -> Result<(), String> {
        let tx = &self.tx;
        let wake = &self.wake;
        #[cfg(feature = "verification-harness")]
        let reject = std::mem::take(&mut self.reject_next);
        self.order.stage(session, update, |value| {
            #[cfg(feature = "verification-harness")]
            if reject {
                return Err("验收注入：归档传输队列满".into());
            }
            tx.try_send(value).map_err(|error| {
                let (description, rejected) = match error {
                    observer_mailbox::TrySendError::Full(value) => {
                        ("日终归档队列已满，当前候选未写入", value)
                    }
                    observer_mailbox::TrySendError::Disconnected(value) => {
                        ("日终归档任务已退出，当前候选未写入", value)
                    }
                };
                libos::deferred_drop::defer(rejected);
                description.to_owned()
            })?;
            wake.notify();
            Ok(())
        })
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
    Skipped {
        seq: String,
        message: String,
    },
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
        let mut consumer = CONSUMER.lock().map_err(|_| error("归档邮箱锁损坏"))?;
        let consumer = consumer.as_mut().ok_or_else(|| error("归档邮箱尚未安装"))?;
        if consumer.writing.is_some() {
            return Err(error("上一份归档尚未报告存储结果"));
        }
        match consumer.rx.try_recv() {
            Ok(value) => {
                if let Err(message) = consumer.fence.begin(value.key) {
                    let seq = value.key.seq.to_string();
                    libos::deferred_drop::defer(value);
                    return to_js(&WireCheckpoint::Skipped { seq, message });
                }
                consumer.writing = Some(value.key);
                value
            }
            Err(TryRecvError::Empty) if !consumer.wake.is_closed() => return Ok(JsValue::NULL),
            Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                return to_js(&WireCheckpoint::Closed)
            }
        }
    };
    // 只在归档 Worker 上编码；锁不跨序列化或持久化。
    to_js(&WireCheckpoint::Checkpoint {
        generation: value.key.generation.to_string(),
        writer_epoch: value.key.writer_epoch.to_string(),
        seq: value.key.seq.to_string(),
        slot: &value.slot,
    })
}

/// 存储副实例先记录本地顺序围栏，再通知权威线程；不依赖控制消息传送速度。
#[wasm_bindgen]
pub fn market_archive_write_result(sequence: u64, failure: Option<String>) -> Result<(), JsValue> {
    let mut consumer = CONSUMER.lock().map_err(|_| error("归档邮箱锁损坏"))?;
    let consumer = consumer.as_mut().ok_or_else(|| error("归档邮箱尚未安装"))?;
    let key = consumer.writing.ok_or_else(|| error("没有在途归档写入"))?;
    if key.seq != sequence {
        return Err(error("归档写入结果不属于在途候选"));
    }
    consumer.writing = None;
    let result = failure.map_or(Ok(()), Err);
    consumer.fence.finish(key, &result);
    Ok(())
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
