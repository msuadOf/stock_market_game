//! LibOS 的浏览器验证入口；仅在 WASM `verification-harness` 构建中公开。
//!
//! 宿主须在 cross-origin isolated 页面使用同一个 `WebAssembly.Memory`
//!（`shared: true`）实例化协调 Worker 与接收 Worker。先由协调 Worker 调用
//! `prepare`，将返回布局传给接收 Worker；接收 Worker 运行期间，协调 Worker
//! 通过本模块的 `send`、`disconnect`、`stop` 或 `abort` 操作发布事件。这些调用
//! 可在不同 Worker 并发执行；同一 session 同时只允许一个接收操作，`finish`
//! 必须等全部 Worker 正常 join 后调用。session 使用本 memory 的 opaque id，
//! 由 Rust registry 验证；不得将任意 JS 数值转换成 Rust 指针。观测 offset 仅在
//! finish 前有效。若 Worker hard trap，
//! Rust 析构和 `finish` 均无保证，宿主必须将实例报告为不可恢复并废弃整块 memory，
//! 不得读取或释放 session 中的业务载荷。共享 `WebAssembly.Memory` 下多个 WASM
//! instance 对 Rust static registry 的初始化与保留尚未由本 API 独立验证；宿主须
//! 确认后续 instance 初始化不会重置 registry。

use super::{downlink, fault_domain, mailbox, worker_wait};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const MODE_MAILBOX: u32 = 0;
const MODE_DOWNLINK: u32 = 1;
const MAX_VERIFICATION_CHANNELS: u32 = 64;
static NEXT_SESSION_ID: AtomicU32 = AtomicU32::new(1);
static SESSION_REGISTRY: Mutex<Vec<(u32, Arc<Session>)>> = Mutex::new(Vec::new());

/// 供 wasm-bindgen/JS wrapper 展开的结果；观测地址是 wasm32 byte offset。
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VerificationLayout {
    /// 由 registry 校验的 opaque id，不是内存地址。
    pub session: u32,
    pub wait_word: u32,
    pub active: u32,
}

struct Session {
    active: AtomicBool,
    active_operations: AtomicU32,
    shutdown: AtomicBool,
    mailbox_sender: Option<Mutex<Option<mailbox::Sender<u32>>>>,
    mailbox_receiver: Option<mailbox::Receiver<u32>>,
    interrupted: Mutex<Option<u32>>,
    fault_domain: Option<Arc<fault_domain::FaultDomain>>,
    downlink_sender: Option<Mutex<Option<downlink::Sender<u32>>>>,
    downlink_receiver: Option<downlink::Receiver<u32>>,
}

// SAFETY：sender/disconnect 由 Mutex 串行；控制状态为原子；故障域自身线程安全。
// mailbox Receiver 的 RefCell 只会在 `active` 成功取得后访问，`run_receive` 与
// `try_receive` 共用同一互斥标记；`abort` 只触碰 FaultDomain，不访问 Receiver。
// finish 的调用方合同要求所有真实 Worker 已 join 后才释放该对象。
unsafe impl Sync for Session {}

/// 一次 API 调用持有的会话租约；finish 只能在租约数归零后摘除 registry 项。
struct SessionLease(Arc<Session>);
impl std::ops::Deref for SessionLease {
    type Target = Session;
    fn deref(&self) -> &Self::Target { &self.0 }
}
impl Drop for SessionLease {
    fn drop(&mut self) {
        let previous = self.0.active_operations.fetch_sub(1, Ordering::Release);
        assert!(previous > 0, "LibOS verification active operation 计数下溢");
    }
}

fn registry() -> Result<std::sync::MutexGuard<'static, Vec<(u32, Arc<Session>)>>, String> {
    match SESSION_REGISTRY.try_lock() {
        Ok(guard) => Ok(guard),
        Err(std::sync::TryLockError::WouldBlock) => Err("LIBOS_VERIFICATION_BUSY: registry 正忙".into()),
        Err(std::sync::TryLockError::Poisoned(_)) => Err("LibOS verification registry 锁中毒".into()),
    }
}

fn lock<T>(mutex: &Mutex<T>, name: &str) -> Result<std::sync::MutexGuard<'_, T>, String> {
    match mutex.try_lock() {
        Ok(guard) => Ok(guard),
        Err(std::sync::TryLockError::WouldBlock) => Err(format!("LIBOS_VERIFICATION_BUSY: {name} 正忙")),
        Err(std::sync::TryLockError::Poisoned(_)) => Err(format!("{name} 锁中毒")),
    }
}

fn lookup(id: u32) -> Result<SessionLease, String> {
    let entries = registry()?;
    let session = entries.iter().find(|(candidate, _)| *candidate == id)
        .map(|(_, session)| Arc::clone(session))
        .ok_or_else(|| "LibOS verification session id 无效、已结束或来自其他实例".to_owned())?;
    session.active_operations.fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| count.checked_add(1))
        .map_err(|_| "LibOS verification active operation 计数溢出".to_owned())?;
    drop(entries);
    Ok(SessionLease(session))
}

/// 已登记的接收调用；正常返回时清除活动标记。hard trap 不会运行其析构。
struct ActiveCall<'a>(&'a AtomicBool);
impl Drop for ActiveCall<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// 创建真实 LibOS 队列及其 host-call 布局。
///
/// `mode=0` 使用 mailbox，可测试通知、断连、故障竞争与超时；`mode=1` 使用
/// 成对 downlink，可测试永久停止时拒绝新发送且不向业务队列插入哨兵。
pub fn prepare(mode: u32) -> Result<VerificationLayout, String> {
    let session = match mode {
        MODE_MAILBOX => {
            let domain = fault_domain::FaultDomain::new(1);
            mailbox_session(&domain)?
        }
        MODE_DOWNLINK => downlink_session(),
        _ => return Err("LibOS verification mode 必须为 0（mailbox）或 1（downlink）".into()),
    };
    register_sessions(vec![Arc::new(session)])?
        .into_iter().next().ok_or_else(|| "LibOS verification session 注册结果为空".into())
}

/// 创建同一真实 `FaultDomain` 下的多个独立 mailbox channel。
///
/// 每个返回项有独立的 opaque session id 和 wait word；对其中任一 session 调用
/// `abort` 会由真实 `FaultDomain` 广播到该组所有已登记 receiver。channel 数限定为
/// 1..=64，只约束验证 fixture 的一次性内存占用，不影响产品通道容量。
pub fn prepare_group(channel_count: u32) -> Result<Vec<VerificationLayout>, String> {
    if !(1..=MAX_VERIFICATION_CHANNELS).contains(&channel_count) {
        return Err(format!("verification channel_count 必须在 1..={MAX_VERIFICATION_CHANNELS}"));
    }
    let capacity = usize::try_from(channel_count)
        .map_err(|_| "verification channel_count 超出平台 usize")?;
    let domain = fault_domain::FaultDomain::new(capacity);
    let mut sessions = Vec::new();
    sessions.try_reserve_exact(capacity)
        .map_err(|error| format!("verification session 分配失败：{error}"))?;
    for _ in 0..channel_count {
        sessions.push(Arc::new(mailbox_session(&domain)?));
    }
    register_sessions(sessions)
}

fn mailbox_session(domain: &Arc<fault_domain::FaultDomain>) -> Result<Session, String> {
    let (sender, receiver) = domain.channel()
        .map_err(|error| format!("LibOS verification 故障域登记失败：{error:?}"))?;
    Ok(Session {
        active: AtomicBool::new(false),
        active_operations: AtomicU32::new(0),
        shutdown: AtomicBool::new(false),
        mailbox_sender: Some(Mutex::new(Some(sender))),
        mailbox_receiver: Some(receiver),
        interrupted: Mutex::new(None),
        fault_domain: Some(Arc::clone(domain)),
        downlink_sender: None,
        downlink_receiver: None,
    })
}

fn downlink_session() -> Session {
    let (sender, receiver) = downlink::channel();
    Session {
        active: AtomicBool::new(false),
        active_operations: AtomicU32::new(0),
        shutdown: AtomicBool::new(false),
        mailbox_sender: None,
        mailbox_receiver: None,
        interrupted: Mutex::new(None),
        fault_domain: None,
        downlink_sender: Some(Mutex::new(Some(sender))),
        downlink_receiver: Some(receiver),
    }
}

fn layout_for(session: &Arc<Session>) -> Result<(u32, u32), String> {
    let wait_word = if let Some(receiver) = &session.mailbox_receiver {
        receiver.abort_handle().wait_word_address()
    } else if let Some(receiver) = &session.downlink_receiver {
        receiver.verification_wait_word_address()
    } else {
        0
    };
    let address = |value: usize, name: &str| {
        u32::try_from(value).map_err(|_| format!("{name} 地址超出 wasm32 shared memory"))
    };
    // wait word 是 mailbox 内部 Signal 的稳定地址；宿主只读取该 AtomicU32 来
    // 确认接收端进入等待，事件发布仍必须调用本模块的 typed 操作。
    let wait_word = if wait_word == 0 { 0 } else { address(wait_word, "wait word")? };
    let active = address(&session.active as *const AtomicBool as usize, "active")?;
    Ok((wait_word, active))
}

fn register_sessions(sessions: Vec<Arc<Session>>) -> Result<Vec<VerificationLayout>, String> {
    let mut observations = Vec::new();
    observations.try_reserve_exact(sessions.len())
        .map_err(|error| format!("verification layout 分配失败：{error}"))?;
    for session in &sessions {
        observations.push(layout_for(session)?);
    }
    let mut entries = registry()?;
    let mut ids = Vec::new();
    ids.try_reserve_exact(sessions.len())
        .map_err(|error| format!("verification id 分配失败：{error}"))?;
    let mut layouts = Vec::new();
    layouts.try_reserve_exact(sessions.len())
        .map_err(|error| format!("verification layout 分配失败：{error}"))?;
    for _ in 0..sessions.len() {
        let id = NEXT_SESSION_ID.fetch_update(Ordering::AcqRel, Ordering::Acquire, |next| next.checked_add(1))
            .map_err(|_| "LibOS verification session id 耗尽".to_owned())?;
        if id == 0 {
            return Err("LibOS verification session id 0 保留".into());
        }
        ids.push(id);
    }
    entries.try_reserve(sessions.len())
        .map_err(|error| format!("LibOS verification registry 分配失败：{error}"))?;
    for (session, (wait_word, active)) in ids.iter().copied().zip(observations) {
        layouts.push(VerificationLayout { session, wait_word, active });
    }
    entries.extend(ids.into_iter().zip(sessions));
    Ok(layouts)
}

/// 向准备好的实际 LibOS 通道发送 u32；失败时返回错误，不丢弃载荷的诊断信息。
pub fn send(session_address: u32, value: u32) -> Result<(), String> {
    let session = lookup(session_address)?;
    if let Some(sender) = &session.mailbox_sender {
        let sender = lock(sender, "mailbox sender")?;
        sender.as_ref().ok_or("mailbox sender 已断开")?.send(value)
            .map_err(|error| format!("mailbox send 失败，原载荷={}", error.0))
    } else if let Some(sender) = &session.downlink_sender {
        let sender = lock(sender, "downlink sender")?;
        sender.as_ref().ok_or("downlink sender 已释放")?.send(value)
            .map_err(|error| format!("downlink send 失败，原载荷={}", error.0))
    } else {
        Err("LibOS verification session 没有发送端".into())
    }
}

/// mailbox 模式销毁最后一个 sender，以验证排空缓冲消息后的断连。
pub fn disconnect(session_address: u32) -> Result<(), String> {
    let session = lookup(session_address)?;
    let Some(sender) = &session.mailbox_sender else {
        return Err("disconnect 仅适用于 mailbox mode".into());
    };
    let sender = lock(sender, "mailbox sender")?
        .take().ok_or("sender 已断开")?;
    drop(sender);
    Ok(())
}

/// 由宿主在另一真实 Worker 中调用，发布故障并唤醒 mailbox 接收 Worker。
pub fn abort(session_address: u32) -> Result<(), String> {
    let session = lookup(session_address)?;
    if session.mailbox_receiver.is_none() {
        return Err("abort 仅适用于 mailbox mode".into());
    }
    session.fault_domain.as_ref().ok_or("mailbox 故障域未登记")?.abort();
    Ok(())
}

/// 由宿主在另一 Worker 发布永久停止；先写 stop，再走真实 downlink 通知。
pub fn stop(session_address: u32) -> Result<(), String> {
    let session = lookup(session_address)?;
    let Some(sender) = &session.downlink_sender else {
        return Err("stop 仅适用于 downlink mode".into());
    };
    // 先取得发送端锁：锁忙时保持 shutdown 原状，使调用方能安全重试，不遗留 PARKED。
    let sender = lock(sender, "downlink sender")?;
    let sender = sender.as_ref().ok_or("downlink sender 已释放")?;
    session.shutdown.store(true, Ordering::SeqCst);
    use super::wake::WorkerWake;
    sender.wake_for_stop();
    Ok(())
}

/// 接收 Worker 的阻塞入口。返回 `(status, value, waited_ms)`：状态码 0=消息、
/// 1=停止、2=断连、3=故障、4=超时；等待时长来自接收端真实时钟，向下取整并
/// 饱和到 `u32::MAX`。状态 3 是 LibOS 可返回故障，不能解读为 trap 可恢复。
pub fn run_receive(
    session_address: u32,
    timeout_millis: u32,
) -> Result<(u32, u32, u32), String> {
    let session = lookup(session_address)?;
    session.active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "同一 LibOS verification session 已有接收 Worker 活动".to_owned())?;
    let _active = ActiveCall(&session.active);
    let started = crate::clock::Instant::now();
    let result = if let Some(receiver) = &session.mailbox_receiver {
        let timeout = Duration::from_millis(u64::from(timeout_millis));
        match mailbox::receive_interruptible(receiver, timeout) {
            Ok(value) => Ok((0, value)),
            Err(mailbox::ReceiveError::Disconnected) => Ok((2, 0)),
            Err(mailbox::ReceiveError::Aborted) => Ok((3, 0)),
            Err(mailbox::ReceiveError::AbortedWithMessage(value)) => {
                let mut interrupted = lock(&session.interrupted, "interrupted 消息")?;
                if interrupted.is_some() {
                    return Err("不可覆盖尚未清理的故障竞争消息".into());
                }
                *interrupted = Some(value);
                Ok((3, 0))
            }
            Err(mailbox::ReceiveError::Timeout { .. }) => Ok((4, 0)),
            Err(mailbox::ReceiveError::ClockRegressed { previous, observed }) => Err(format!(
                "mailbox clock 回退：previous={previous:?}, observed={observed:?}"
            )),
        }
    } else if let Some(receiver) = &session.downlink_receiver {
        if timeout_millis != 0 {
            return Err("downlink worker receive 是永久等待；timeout_millis 必须为 0".into());
        }
        match receiver.receive(&session.shutdown) {
            Ok(Some(value)) => Ok((0, value)),
            Ok(None) => Ok((1, 0)),
            Err(worker_wait::WorkerReceiveError::Disconnected) => Ok((2, 0)),
            Err(worker_wait::WorkerReceiveError::HostAborted) => Ok((3, 0)),
            Err(error) => Err(format!("downlink worker receive 失败：{error:?}")),
        }
    } else {
        Err("LibOS verification session 没有接收端".into())
    }?;
    let elapsed = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
    Ok((result.0, result.1, elapsed))
}

/// 保留业务 phase 的 coordinator deadline 入口。phase 编码：0=`TickConverge`、
/// 1=`Seal`、2=`Drain`、3=`FinalCollection`。`normal_ms` 适用于前两者，
/// `long_ms` 适用于后两者；返回 `(status, value, waited_ms, phase)`，timeout 时
/// 仍回传实际耗时与原 phase，供宿主映射带上下文的失败。期限使用 LibOS 的真实时钟。
pub fn run_phase_receive(
    session_address: u32,
    phase: u32,
    normal_ms: u32,
    long_ms: u32,
) -> Result<(u32, u32, u32, u32), String> {
    let timeout = match phase {
        0 | 1 => normal_ms,
        2 | 3 => long_ms,
        _ => return Err("verification phase 必须为 0..=3".into()),
    };
    let (status, value, waited) = run_receive(session_address, timeout)?;
    Ok((status, value, waited, phase))
}

/// 读取实际队列中的下一条消息；用于检查 stop/fault 后未交付载荷仍然保留。
pub fn try_receive(session_address: u32) -> Result<(u32, u32), String> {
    let session = lookup(session_address)?;
    session.active.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .map_err(|_| "接收 Worker 仍活动，try_receive 不得并发访问 Receiver".to_owned())?;
    let _active = ActiveCall(&session.active);
    if let Some(receiver) = &session.mailbox_receiver {
        if let Some(value) = lock(&session.interrupted, "interrupted 消息")?.take() {
            return Ok((0, value));
        }
        match receiver.try_recv() {
            Ok(value) => Ok((0, value)),
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok((1, 0)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Ok((2, 0)),
        }
    } else if let Some(receiver) = &session.downlink_receiver {
        match receiver.try_receive().map_err(|error| format!("downlink try_receive 故障：{error:?}"))? {
            Ok(value) => Ok((0, value)),
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok((1, 0)),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Ok((2, 0)),
        }
    } else {
        Err("LibOS verification session 没有接收端".into())
    }
}

/// 正常路径在全部 Worker join 后调用；仍有活动操作时保留 registry entry 供重试。
pub fn finish(session_id: u32) -> Result<(), String> {
    let mut entries = registry()?;
    let index = entries.iter().position(|(candidate, _)| *candidate == session_id)
        .ok_or_else(|| "LibOS verification session id 无效、已结束或来自其他实例".to_owned())?;
    let session = &entries[index].1;
    if session.active_operations.load(Ordering::Acquire) != 0 || session.active.load(Ordering::Acquire) {
        return Err("仍有 LibOS verification 操作活动；确认 Worker join 后重试 finish".into());
    }
    let removed = entries.remove(index);
    drop(entries);
    drop(removed);
    Ok(())
}
