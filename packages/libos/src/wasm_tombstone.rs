//! 每 Worker 固定 256B 诊断记录与 WASM panic 早期通知（设计 §8.6-②）。
//!
//! 记录在执行前预分配并绑定 Worker 身份；死亡通知先于尽力发布记录。
//! 记录禁止复用或重置，直至整个共享 memory 废弃才回收。硬 trap 和绑定前
//! 失败可能没有记录，宿主必须保留这些缺席状态。
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicU8, Ordering};

const IDLE: u32 = 0;
const WRITING: u32 = 1;
const LIVE: u32 = 2;
pub const PAYLOAD_BYTES: usize = 240;

/// 所有字段为原子字节，允许 JS 观察半发布状态而不引入 Rust 数据竞争。
#[repr(C)]
pub struct TombstoneRecord {
    magic: AtomicU32,
    worker_id: AtomicU32,
    phase: AtomicU16,
    kind: AtomicU8,
    truncated: AtomicU8,
    len: AtomicU16,
    reserved: AtomicU16,
    payload: [AtomicU8; PAYLOAD_BYTES],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DeathKind {
    Panic = 1,
    HardTrap = 2,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TombstoneHeader {
    pub worker_id: u32,
    pub phase: u16,
    pub kind: DeathKind,
}
#[derive(Debug, PartialEq, Eq)]
pub struct TombstoneSnapshot {
    pub header: TombstoneHeader,
    pub truncated: bool,
    pub len: u16,
    pub payload: [u8; PAYLOAD_BYTES],
}
/// 无等待读取；缺席、在写与损坏均不能被编造成 panic payload。
#[derive(Debug, PartialEq, Eq)]
pub enum TombstoneRead {
    Idle,
    Writing,
    Live(TombstoneSnapshot),
    Invalid(TombstoneInvalid),
}
#[derive(Debug, PartialEq, Eq)]
pub enum TombstoneInvalid {
    Magic(u32),
    Kind(u8),
    Length(u16),
    Truncated(u8),
    TruncatedLength(u16),
    Reserved(u16),
}
/// 不可 Clone；唯一 CAS 获胜者持有写权，遗弃后永久停留 WRITING。
pub struct TombstoneWriter<'a> {
    record: &'a TombstoneRecord,
    header: TombstoneHeader,
}

impl TombstoneRecord {
    pub const fn new() -> Self {
        Self {
            magic: AtomicU32::new(IDLE),
            worker_id: AtomicU32::new(0),
            phase: AtomicU16::new(0),
            kind: AtomicU8::new(0),
            truncated: AtomicU8::new(0),
            len: AtomicU16::new(0),
            reserved: AtomicU16::new(0),
            payload: [const { AtomicU8::new(0) }; PAYLOAD_BYTES],
        }
    }

    /// 先发送死亡信号，再尽力发布记录；已有写者时不得覆盖。
    pub fn signal_then_publish(
        &self,
        header: TombstoneHeader,
        payload: &[u8],
        signal: impl FnOnce(),
    ) {
        signal();
        if let Some(writer) = self.try_claim(header) {
            writer.publish(payload);
        }
    }

    pub fn try_claim(&self, header: TombstoneHeader) -> Option<TombstoneWriter<'_>> {
        self.magic
            .compare_exchange(IDLE, WRITING, Ordering::AcqRel, Ordering::Acquire)
            .ok()
            .map(|_| TombstoneWriter {
                record: self,
                header,
            })
    }

    pub fn read(&self) -> TombstoneRead {
        // 定稿要求发布字 SeqCst，随后字段读取由此 Acquire 保证可见。
        match self.magic.load(Ordering::SeqCst) {
            IDLE => return TombstoneRead::Idle,
            WRITING => return TombstoneRead::Writing,
            LIVE => {}
            value => return TombstoneRead::Invalid(TombstoneInvalid::Magic(value)),
        }
        let kind = match self.kind.load(Ordering::Relaxed) {
            1 => DeathKind::Panic,
            2 => DeathKind::HardTrap,
            value => return TombstoneRead::Invalid(TombstoneInvalid::Kind(value)),
        };
        let len = self.len.load(Ordering::Relaxed);
        if len as usize > PAYLOAD_BYTES {
            return TombstoneRead::Invalid(TombstoneInvalid::Length(len));
        }
        let truncated = self.truncated.load(Ordering::Relaxed);
        if truncated > 1 {
            return TombstoneRead::Invalid(TombstoneInvalid::Truncated(truncated));
        }
        if truncated == 1 && len as usize != PAYLOAD_BYTES {
            return TombstoneRead::Invalid(TombstoneInvalid::TruncatedLength(len));
        }
        let reserved = self.reserved.load(Ordering::Relaxed);
        if reserved != 0 {
            return TombstoneRead::Invalid(TombstoneInvalid::Reserved(reserved));
        }
        TombstoneRead::Live(TombstoneSnapshot {
            header: TombstoneHeader {
                worker_id: self.worker_id.load(Ordering::Relaxed),
                phase: self.phase.load(Ordering::Relaxed),
                kind,
            },
            len,
            truncated: truncated == 1,
            payload: std::array::from_fn(|index| self.payload[index].load(Ordering::Relaxed)),
        })
    }
}
/// child 提前绑定的 join 与记录；失败路径只做有界原子操作。
#[derive(Clone, Copy)]
pub struct PanicTarget<'a> {
    pub record: &'a TombstoneRecord,
    pub join: &'a AtomicU32,
    pub dead_code: &'a AtomicU32,
    pub waiting: u32,
    pub writing: u32,
    pub dead: u32,
    pub header: TombstoneHeader,
}

impl PanicTarget<'_> {
    pub fn publish(&self, payload: &[u8], notify: impl FnOnce()) {
        self.record.signal_then_publish(self.header, payload, || {
            if self
                .join
                .compare_exchange(
                    self.waiting,
                    self.writing,
                    Ordering::SeqCst,
                    Ordering::SeqCst,
                )
                .is_ok()
            {
                self.dead_code.store(1, Ordering::SeqCst);
                if self
                    .join
                    .compare_exchange(self.writing, self.dead, Ordering::SeqCst, Ordering::SeqCst)
                    .is_ok()
                {
                    notify();
                }
            }
        });
    }
}
impl Default for TombstoneRecord {
    fn default() -> Self {
        Self::new()
    }
}
impl TombstoneWriter<'_> {
    /// 原始字节有界复制，不做 UTF-8 解码、格式化、分配或加锁。
    pub fn publish(self, payload: &[u8]) {
        let record = self.record;
        let len = payload.len().min(PAYLOAD_BYTES);
        record
            .worker_id
            .store(self.header.worker_id, Ordering::Release);
        record.phase.store(self.header.phase, Ordering::Release);
        record.kind.store(self.header.kind as u8, Ordering::Release);
        record
            .truncated
            .store(u8::from(payload.len() > PAYLOAD_BYTES), Ordering::Release);
        record.len.store(len as u16, Ordering::Release);
        for (cell, byte) in record.payload.iter().zip(payload).take(PAYLOAD_BYTES) {
            cell.store(*byte, Ordering::Release);
        }
        record.magic.store(LIVE, Ordering::SeqCst);
    }
}

#[cfg(target_arch = "wasm32")]
mod early {
    use super::*;
    use std::cell::Cell;
    use std::sync::OnceLock;

    #[derive(Clone, Copy)]
    enum EarlyTarget {
        Child(PanicTarget<'static>),
        Primary {
            record: &'static TombstoneRecord,
            worker_id: u32,
        },
    }
    struct PrimaryRecords {
        engine: &'static TombstoneRecord,
        ingress: &'static TombstoneRecord,
    }
    static PRIMARY: OnceLock<PrimaryRecords> = OnceLock::new();
    const ENGINE_ID: u32 = 0;
    const INGRESS_ID: u32 = u32::MAX;
    static DEAD: AtomicU32 = AtomicU32::new(0);
    thread_local! {
        static TARGET: Cell<Option<EarlyTarget>> = const { Cell::new(None) };
    }

    /// 在运行闭包前绑定；调用方保证 arena 与记录持续到整个实例废弃。
    pub fn bind(target: PanicTarget<'static>) {
        TARGET.with(|slot| {
            assert!(slot.get().is_none(), "同一 Worker 不能重复绑定 tombstone");
            slot.set(Some(EarlyTarget::Child(target)));
        });
    }

    /// 初始化独立 memory 的两个主 Worker 记录，并绑定当前 engine TLS。
    pub fn init_primary() -> Result<(u32, u32, u32), &'static str> {
        if PRIMARY.get().is_some() || TARGET.with(|slot| slot.get().is_some()) {
            return Err("主 Worker tombstone 已初始化或 TLS 已绑定");
        }
        let records = PrimaryRecords {
            engine: Box::leak(Box::new(TombstoneRecord::new())),
            ingress: Box::leak(Box::new(TombstoneRecord::new())),
        };
        let engine = records.engine;
        let ingress = records.ingress;
        PRIMARY
            .set(records)
            .map_err(|_| "主 Worker tombstone 并发重复初始化")?;
        TARGET.with(|slot| {
            slot.set(Some(EarlyTarget::Primary {
                record: engine,
                worker_id: ENGINE_ID,
            }))
        });
        Ok((
            engine as *const _ as u32,
            ingress as *const _ as u32,
            &DEAD as *const _ as u32,
        ))
    }

    /// ingress 只能绑定同一共享 memory 预登记的记录及保留身份。
    pub fn bind_ingress(address: u32, worker_id: u32) -> Result<(), &'static str> {
        let records = PRIMARY.get().ok_or("主 Worker tombstone 尚未初始化")?;
        if worker_id != INGRESS_ID || address != records.ingress as *const _ as u32 {
            return Err("ingress tombstone 地址或身份不匹配");
        }
        TARGET.with(|slot| {
            if slot.get().is_some() {
                return Err("ingress TLS 已绑定 tombstone");
            }
            slot.set(Some(EarlyTarget::Primary {
                record: records.ingress,
                worker_id,
            }));
            Ok(())
        })
    }

    /// 宿主读取全局死亡字的共享地址。
    pub fn death_word() -> *const AtomicU32 {
        &DEAD
    }

    /// 由受控 std 在 panic_count 和标准 hook 前调用；不格式化或分配。
    #[no_mangle]
    pub unsafe extern "C" fn wasm_tombstone_early(ptr: *const u8, len: usize, has_message: bool) {
        DEAD.store(1, Ordering::SeqCst);
        core::arch::wasm32::memory_atomic_notify(
            (&DEAD as *const AtomicU32).cast_mut().cast::<i32>(),
            u32::MAX,
        );
        TARGET.with(|slot| {
            if let Some(target) = slot.get() {
                let bytes = if has_message {
                    std::slice::from_raw_parts(ptr, len)
                } else {
                    "panic 消息不可用".as_bytes()
                };
                match target {
                    EarlyTarget::Child(target) => target.publish(bytes, || {
                        core::arch::wasm32::memory_atomic_notify(
                            (target.join as *const AtomicU32).cast_mut().cast::<i32>(),
                            u32::MAX,
                        );
                    }),
                    EarlyTarget::Primary { record, worker_id } => {
                        if let Some(writer) = record.try_claim(TombstoneHeader {
                            worker_id,
                            phase: 0,
                            kind: DeathKind::Panic,
                        }) {
                            writer.publish(bytes);
                        }
                    }
                }
            }
        });
    }
}

#[cfg(target_arch = "wasm32")]
pub use early::{bind, bind_ingress, death_word, init_primary};

#[cfg(test)]
#[path = "wasm_tombstone_tests.rs"]
mod tests;
