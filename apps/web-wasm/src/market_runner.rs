//! 浏览器会话 ABI：权威端只向 Rust 邮箱交接；发布 Worker 独占出队与序列化。
use super::{to_js, HostFailure, REGISTRY};
use engine::session::authority::{self, Authority as Pacing, AuthorityPort};
use engine::session::{
    protocol::{CivilUpdate, EngineUpdate, PausePreferences, ProtocolSession, TickBatch},
    publication::{self, ObserverPump, Producer, PumpRead, Read, ResumeError},
    runner::{self, DrivePort, WakePlan},
};
use libos::{clock::Instant, wake_epoch::WakeEpoch};
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};
use wasm_bindgen::prelude::*;

use engine::session::publication::lifecycle::{ObserverLease, ObserverLifecycle, TerminalDelivery};

mod checkpoint;
use checkpoint::ArchivePort;

enum WorkerOutput {
    Update {
        update: EngineUpdate,
        barrier_paused: bool,
    },
    Recovery(HostFailure),
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum WireOutput {
    #[serde(rename_all = "camelCase")]
    Update {
        update: EngineUpdate,
        barrier_paused: bool,
        publication_ordinal: String,
    },
    #[serde(rename_all = "camelCase")]
    Gap {
        oldest_after: String,
    },
    #[serde(rename_all = "camelCase")]
    Failure {
        failure: HostFailure,
        #[serde(skip_serializing_if = "Option::is_none")]
        publication_ordinal: Option<String>,
    },
    Closed,
}

struct Drive {
    generation: u64,
    pacing: Pacing,
    output: Producer<WorkerOutput>,
    control: Arc<WakeEpoch>,
    handled_control: i32,
    pause: PausePreferences,
    terminal: Arc<OnceLock<Terminal>>,
    archive: Option<ArchivePort>,
}
struct Outbound {
    pump: ObserverPump<WorkerOutput>,
    terminal: Arc<OnceLock<Terminal>>,
    terminal_delivery: TerminalDelivery,
    lifecycle: ObserverLifecycle,
    control: Arc<WakeEpoch>,
}
struct Terminal {
    after: u64,
    failure: HostFailure,
}
type SharedOutbound = Arc<Mutex<Outbound>>;
static OUTBOUND: Mutex<Option<HashMap<u32, SharedOutbound>>> = Mutex::new(None);
thread_local! {
    static DRIVES: RefCell<HashMap<u32, Drive>> = RefCell::new(HashMap::new());
}

fn outbound(token: u32) -> Result<SharedOutbound, JsValue> {
    OUTBOUND
        .lock()
        .map_err(|_| error("发布端 registry 已中毒"))?
        .as_ref()
        .and_then(|all| all.get(&token))
        .cloned()
        .ok_or_else(|| error("未知或已释放的发布令牌"))
}

fn error(message: &str) -> JsValue {
    JsValue::from_str(message)
}

#[wasm_bindgen]
pub fn market_runner_enable(handle: u32) -> Result<(), JsValue> {
    REGISTRY.with(|registry| {
        let (tick, generation) = {
            let registry = registry.borrow();
            let session = registry
                .sessions
                .get(&handle)
                .ok_or_else(|| error("未知的市场推进会话"))?;
            let generation = *registry
                .generations
                .get(&handle)
                .ok_or_else(|| error("市场会话缺少 generation"))?;
            (session.tick(), generation)
        };
        DRIVES.with(|drives| {
            let mut drives = drives.borrow_mut();
            if drives.contains_key(&handle) {
                return Ok(());
            }
            let mut registry = OUTBOUND
                .lock()
                .map_err(|_| error("发布端 registry 已中毒"))?;
            let all = registry.get_or_insert_with(HashMap::new);
            if all.contains_key(&handle) {
                return Err(error("市场发布令牌重复"));
            }
            let (output, observer) = publication::channel(publication::OBSERVER_PACKET_LIMIT);
            let control = Arc::new(WakeEpoch::new());
            let terminal = Arc::new(OnceLock::new());
            all.insert(
                handle,
                Arc::new(Mutex::new(Outbound {
                    pump: ObserverPump::new(observer, publication::OBSERVER_IN_FLIGHT_LIMIT),
                    terminal: Arc::clone(&terminal),
                    terminal_delivery: TerminalDelivery::default(),
                    lifecycle: ObserverLifecycle::default(),
                    control: Arc::clone(&control),
                })),
            );
            drives.insert(
                handle,
                Drive {
                    generation,
                    pacing: Pacing::new(runner::BASE_TICK_MS, tick, Instant::now()),
                    output,
                    handled_control: control.load(),
                    control,
                    pause: PausePreferences::default(),
                    terminal,
                    archive: None,
                },
            );
            Ok(())
        })
    })
}

/// prepare 阶段仅建立发布/调度适配，不安装第二份归档 CONSUMER。
pub(super) fn prepare_replacement_drive(previous: u32, candidate: u32) -> Result<(), JsValue> {
    let settings = with_drive(previous, |drive| {
        Ok(engine::session::replacement::RestoreSettings::capture(
            &drive.pacing,
            drive.pause,
        ))
    })?;
    market_runner_enable(candidate)?;
    let tick = session_tick(candidate)?;
    with_drive(candidate, |drive| {
        settings.install(&mut drive.pacing, tick, Instant::now());
        drive.pause = settings.pause;
        Ok(())
    })
}

pub(super) fn discard_prepared_drive(handle: u32) -> Result<(), engine::SessionError> {
    let mut all = OUTBOUND
        .lock()
        .map_err(|_| engine::SessionError::ResourceLimit(engine::ValidationFault::bare(1293)))?;
    if let Some(all) = all.as_mut() {
        all.remove(&handle);
    }
    drop_drive(handle);
    Ok(())
}

/// 不可逆退役旧 authority 时停止 Rust 的运行意图，不依赖 JS loop 状态。
pub(super) fn fence_replacement_drive(handle: u32) -> Result<(), JsValue> {
    let tick = session_tick(handle)?;
    with_drive(handle, |drive| {
        drive.pacing.set_running(false, tick, Instant::now());
        Ok(())
    })
}

pub(super) fn commit_replacement_generation(handle: u32, generation: u64) {
    with_drive(handle, |drive| {
        drive.generation = generation;
        Ok(())
    })
    .expect("prepare 已建立同一 registry 生命周期的 Drive");
}

/// 在放行市场推进前安装有界通道；旧世代先退场。
#[wasm_bindgen]
pub fn market_archive_attach(
    handle: u32,
    generation: u64,
    writer_epoch: u64,
) -> Result<(), JsValue> {
    with_drive(handle, |drive| {
        if drive.archive.is_some() || generation == 0 || writer_epoch == 0 {
            return Err(error("归档身份无效或通道重复安装"));
        }
        drive.archive = Some(ArchivePort::open(generation, writer_epoch)?);
        Ok(())
    })
}

#[wasm_bindgen]
pub fn market_archive_detach(handle: u32) -> Result<(), JsValue> {
    with_drive(handle, |drive| {
        if let Some(archive) = &drive.archive {
            archive.close();
        }
        Ok(())
    })
}

#[wasm_bindgen]
pub fn market_archive_latest(handle: u32) -> Result<Option<String>, JsValue> {
    with_drive(handle, |drive| {
        Ok(drive
            .archive
            .as_ref()
            .ok_or_else(|| error("归档通道未安装"))?
            .latest_staged()
            .map(|seq| seq.to_string()))
    })
}

#[wasm_bindgen]
pub fn market_archive_receipt(
    handle: u32,
    generation: u64,
    writer_epoch: u64,
    sequence: u64,
    failure: Option<String>,
) -> Result<String, JsValue> {
    REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let session = registry
            .sessions
            .get(&handle)
            .ok_or_else(|| error("未知的归档会话"))?;
        with_drive(handle, |drive| {
            let Some(archive) = drive.archive.as_mut() else {
                return Ok("stale".into());
            };
            if archive.generation != generation || archive.writer_epoch != writer_epoch {
                return Ok("stale".into());
            }
            let result = archive.receive(sequence, failure.map_or(Ok(()), Err));
            let failure = match result {
                Ok(engine::session::checkpoint::ReceiptOutcome::Durable) => {
                    return Ok("durable".into())
                }
                Ok(engine::session::checkpoint::ReceiptOutcome::Stale) => return Ok("stale".into()),
                Ok(engine::session::checkpoint::ReceiptOutcome::Failed { key, message }) => {
                    let mut failure = HostFailure::archive(&message);
                    failure.with_archive_key(key);
                    failure
                }
                Err(message) => HostFailure::archive(&message),
            };
            publish_authority_failure(session, drive, failure)?;
            Ok("failed".into())
        })
    })
}

#[wasm_bindgen]
pub fn market_archive_failed(
    handle: u32,
    generation: u64,
    writer_epoch: u64,
    message: String,
) -> Result<(), JsValue> {
    REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let session = registry
            .sessions
            .get(&handle)
            .ok_or_else(|| error("未知的归档会话"))?;
        with_drive(handle, |drive| {
            let Some(archive) = drive.archive.as_mut() else {
                return Ok(());
            };
            if archive.generation != generation || archive.writer_epoch != writer_epoch {
                return Ok(());
            }
            archive.block(message.clone());
            publish_authority_failure(session, drive, HostFailure::archive(&message))
        })
    })
}

/// close 先停止入队，存储副实例退出后才消费完毕并移除权威归档状态。
#[wasm_bindgen]
pub fn market_archive_finish(handle: u32, abandon: bool) -> Result<(), JsValue> {
    with_drive(handle, |drive| {
        let Some(archive) = drive.archive.as_ref() else {
            return Err(error("归档通道未安装"));
        };
        if !abandon {
            archive.ensure_drained().map_err(|reason| error(&reason))?;
        }
        drop(drive.archive.take());
        Ok(())
    })
}

pub(super) fn drop_drive(handle: u32) {
    DRIVES.with(|drives| {
        drives.borrow_mut().remove(&handle);
    });
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct WorkerBaseline {
    snapshot: engine::Snapshot,
    working_orders: Vec<engine::session::protocol::PlayerWorkingOrder>,
    publication_ordinal: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    latest_checkpoint: Option<engine::session::protocol::SaveCandidateKey>,
    stock_catalog: Vec<engine::session::StockIdentityEntry>,
    company_catalog: Vec<engine::session::CompanyCatalogEntry>,
    civil_date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    failure: Option<HostFailure>,
}

#[wasm_bindgen]
pub fn market_runner_observer_baseline(handle: u32) -> Result<JsValue, JsValue> {
    REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let session = registry
            .sessions
            .get(&handle)
            .ok_or_else(|| error("未知的市场推进会话"))?;
        let failure = with_drive(handle, |drive| Ok(drive.pacing.latest_failure().cloned()))?;
        let captured = match engine::session::baseline::CapturedBaseline::capture(
            session,
            Some(engine::AccountId(0)),
            failure,
        ) {
            Ok(captured) => captured,
            Err(source) => {
                let failure =
                    HostFailure::catalog(&source, "observerBaseline", "session.authority.baseline")
                        .at_session(session);
                with_drive(handle, |drive| {
                    publish_authority_failure(session, drive, failure.clone())
                })?;
                return Err(super::step_update_error_to_js(
                    super::StepUpdateError::Fatal(Box::new(failure)),
                ));
            }
        };
        if captured.discovered_failure {
            if let Some(failure) = captured.failure.clone() {
                with_drive(handle, |drive| {
                    publish_authority_failure(session, drive, failure)
                })?;
            }
        }
        let publication_ordinal = market_runner_publication_ordinal(handle)?;
        to_js(&WorkerBaseline {
            snapshot: captured.snapshot,
            working_orders: captured.working_orders,
            publication_ordinal,
            latest_checkpoint: captured.latest_checkpoint,
            stock_catalog: captured.stock_catalog,
            company_catalog: captured.company_catalog,
            civil_date: captured.civil_date.to_iso(),
            failure: with_drive(handle, |drive| Ok(drive.pacing.latest_failure().cloned()))?,
        })
    })
}

fn with_drive<T>(
    handle: u32,
    use_drive: impl FnOnce(&mut Drive) -> Result<T, JsValue>,
) -> Result<T, JsValue> {
    DRIVES.with(|drives| {
        let mut drives = drives.borrow_mut();
        use_drive(
            drives
                .get_mut(&handle)
                .ok_or_else(|| error("市场推进尚未启用"))?,
        )
    })
}

fn session_tick(handle: u32) -> Result<u64, JsValue> {
    REGISTRY.with(|registry| {
        registry
            .borrow()
            .sessions
            .get(&handle)
            .map(|session| session.tick())
            .ok_or_else(|| error("未知的市场推进会话"))
    })
}

#[wasm_bindgen]
pub fn market_runner_set_running(handle: u32, running: bool) -> Result<(), JsValue> {
    let tick = session_tick(handle)?;
    with_drive(handle, |drive| {
        drive
            .pacing
            .control_running(
                running,
                tick,
                Instant::now(),
                drive
                    .archive
                    .as_ref()
                    .and_then(|archive| archive.blocked_reason()),
            )
            .map_err(|source| error(&source.to_string()))
    })
}

#[wasm_bindgen]
pub fn market_runner_set_speed(handle: u32, multiplier: f64) -> Result<(), JsValue> {
    let tick = session_tick(handle)?;
    with_drive(handle, |drive| {
        drive
            .pacing
            .control_speed(
                multiplier,
                tick,
                Instant::now(),
                drive
                    .archive
                    .as_ref()
                    .and_then(|archive| archive.blocked_reason()),
            )
            .map_err(|source| error(&source.to_string()))
    })
}

#[wasm_bindgen]
pub fn market_runner_speed_metrics(handle: u32) -> Result<JsValue, JsValue> {
    let tick = session_tick(handle)?;
    with_drive(handle, |drive| {
        to_js(&drive.pacing.refresh_metrics(tick, Instant::now()))
    })
}

#[wasm_bindgen]
pub fn market_runner_set_pause(
    handle: u32,
    after_close: bool,
    before_open: bool,
) -> Result<(), JsValue> {
    with_drive(handle, |drive| {
        drive.pause = drive
            .pacing
            .control_pause(PausePreferences {
                pause_after_close: after_close,
                pause_before_open: before_open,
            })
            .map_err(|source| error(&source.to_string()))?;
        Ok(())
    })
}

#[wasm_bindgen]
pub fn market_runner_command_consumed(handle: u32) -> Result<(), JsValue> {
    with_drive(handle, |drive| {
        if drive.handled_control == drive.control.load() {
            return Err(error("没有待确认的市场控制命令"));
        }
        drive.handled_control = drive.handled_control.wrapping_add(1);
        Ok(())
    })
}

struct DriverPort<'a> {
    session: &'a mut ProtocolSession,
    drive: &'a mut Drive,
}
impl DrivePort for DriverPort<'_> {
    type Error = String;

    fn session(&self) -> &ProtocolSession {
        self.session
    }
    fn session_mut(&mut self) -> &mut ProtocolSession {
        self.session
    }
    fn command_pending(&self) -> bool {
        self.drive.handled_control != self.drive.control.load()
    }
    fn stage_tick_batch(&mut self, batch: TickBatch) -> Result<(), Self::Error> {
        stage_update(self.drive, EngineUpdate::TickBatch(Box::new(batch)), false);
        Ok(())
    }
    fn stage_civil(
        &mut self,
        update: CivilUpdate,
        barrier_paused: bool,
    ) -> Result<(), Self::Error> {
        stage_civil_checkpoint(self.session, self.drive, update, barrier_paused)
    }
}

fn stage_civil_checkpoint(
    session: &ProtocolSession,
    drive: &mut Drive,
    update: CivilUpdate,
    paused: bool,
) -> Result<(), String> {
    let staged = match &mut drive.archive {
        Some(archive) => archive.try_stage(session, &update),
        None => Ok(()), // 独立绑定 ABI 验证可以没有持久化端点。
    };
    stage_update(drive, EngineUpdate::CivilUpdate(Box::new(update)), paused);
    staged
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TurnResult {
    delay_ms: Option<f64>,
    running: bool,
    committed_ticks: u64,
}

#[wasm_bindgen]
pub fn market_runner_turn(handle: u32) -> Result<JsValue, JsValue> {
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let session = registry
            .sessions
            .get_mut(&handle)
            .ok_or_else(|| error("未知的市场推进会话"))?;
        with_drive(handle, |drive| {
            let before = session.tick();
            if drive.handled_control == drive.control.load() {
                if let Some(permit) = drive.pacing.cadence.begin_turn(Instant::now()) {
                    let pause = drive.pause;
                    if let Some(failure) =
                        authority::drive(&mut DriverPort { session, drive }, permit, pause)
                    {
                        publish_authority_failure(session, drive, failure)?;
                    }
                }
            }
            let delay_ms = match drive.pacing.cadence.wake_plan() {
                WakePlan::Idle => None,
                WakePlan::Immediate => Some(0.0),
                WakePlan::At(deadline) => Some(
                    deadline
                        .saturating_duration_since(Instant::now())
                        .as_secs_f64()
                        * 1_000.0,
                ),
            };
            to_js(&TurnResult {
                delay_ms,
                running: drive.pacing.is_running(),
                committed_ticks: session.tick() - before,
            })
        })
    })
}

/// 产品验收的手工步进只交付一个 tick；日界必须由独立的显式命令推进。
#[wasm_bindgen]
pub fn market_runner_step_once(handle: u32) -> Result<(), JsValue> {
    with_manual_authority(handle, |session, drive| {
        let outcome = authority::manual_tick(&mut DriverPort { session, drive });
        if let Some(failure) = outcome.failure {
            publish_authority_failure(session, drive, failure)?;
        }
        outcome
            .result
            .map(|_| ())
            .map_err(|source| error(&source.to_string()))
    })
}

#[wasm_bindgen]
pub fn market_runner_end_civil(handle: u32) -> Result<(), JsValue> {
    with_manual_authority(handle, |session, drive| {
        let pause = drive.pause;
        let outcome = authority::manual_civil(&mut DriverPort { session, drive }, pause);
        if let Some(failure) = outcome.failure {
            publish_authority_failure(session, drive, failure)?;
        }
        outcome
            .result
            .map(|_| ())
            .map_err(|source| error(&source.to_string()))
    })
}

fn with_manual_authority(
    handle: u32,
    operation: impl FnOnce(&mut ProtocolSession, &mut Drive) -> Result<(), JsValue>,
) -> Result<(), JsValue> {
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let session = registry
            .sessions
            .get_mut(&handle)
            .ok_or_else(|| error("未知的市场推进会话"))?;
        with_drive(handle, |drive| operation(session, drive))
    })
}

fn publish_authority_failure(
    session: &ProtocolSession,
    drive: &mut Drive,
    failure: HostFailure,
) -> Result<(), JsValue> {
    let generation = drive.generation;
    let Some(failure) = drive
        .pacing
        .fail(session, generation, failure, Instant::now())
    else {
        return Ok(());
    };
    if failure.recoverable {
        drive.output.publish(WorkerOutput::Recovery(failure));
    } else {
        record_terminal(drive, failure)?;
    }
    Ok(())
}

fn stage_update(drive: &mut Drive, update: EngineUpdate, barrier_paused: bool) {
    let weight = publication::protocol_weight(&update);
    drive.output.publish_weighted(
        WorkerOutput::Update {
            update,
            barrier_paused,
        },
        weight,
    );
}

#[wasm_bindgen]
pub fn market_runner_publication_ordinal(handle: u32) -> Result<String, JsValue> {
    with_drive(handle, |drive| {
        Ok(drive.output.published_ordinal().to_string())
    })
}

/// 导出仅负责把同一 Memory 生存期内保持存活的 LibOS 地址交给 transport。
#[wasm_bindgen]
pub fn market_runner_control_signal(token: u32) -> Result<u32, JsValue> {
    let binding = outbound(token)?;
    let binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
    binding
        .control
        .shared_memory_offset()
        .ok_or_else(|| error("当前运行时不支持共享线性内存通知"))
}

#[wasm_bindgen]
pub fn market_publisher_signal(token: u32) -> Result<u32, JsValue> {
    let binding = outbound(token)?;
    let binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
    binding
        .pump
        .signal()
        .shared_memory_offset()
        .ok_or_else(|| error("当前运行时不支持共享线性内存通知"))
}

/// 真实 Outbound 的只读验收状态；生产 ABI 不导出此入口。
#[cfg(any(test, feature = "verification-harness"))]
pub(crate) fn inspect_observer(
    token: u32,
) -> Result<publication::lifecycle::ObserverEvidence, JsValue> {
    let binding = outbound(token)?;
    let binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
    Ok(binding.lifecycle.snapshot())
}

#[wasm_bindgen]
pub fn market_publisher_bind(token: u32, generation: u64) -> Result<u64, JsValue> {
    let binding = outbound(token)?;
    let mut binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
    let lease = binding.lifecycle.hold(generation);
    binding.lifecycle.fence(lease, 0);
    binding.lifecycle.release(lease);
    Ok(lease.lease)
}

#[wasm_bindgen]
pub fn market_publisher_hold(token: u32, lease: u64) -> Result<u64, JsValue> {
    let binding = outbound(token)?;
    let mut binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
    let current = require_lease(&binding, lease)?;
    Ok(binding.lifecycle.hold(current.generation).lease)
}

#[wasm_bindgen]
pub fn market_publisher_start(token: u32, lease: u64) -> Result<(), JsValue> {
    let binding = outbound(token)?;
    let mut binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
    let current = require_lease(&binding, lease)?;
    if !binding.lifecycle.release(current) {
        return Err(error("观察者尚未安装当前 lease 的基线"));
    }
    Ok(())
}

fn require_lease(binding: &Outbound, lease: u64) -> Result<ObserverLease, JsValue> {
    binding
        .lifecycle
        .current()
        .filter(|current| current.lease == lease)
        .ok_or_else(|| error("STALE_OBSERVER_LEASE"))
}

#[wasm_bindgen]
pub fn market_publisher_take(token: u32, lease: u64) -> Result<JsValue, JsValue> {
    let binding = outbound(token)?;
    // 序列化不持有 registry/observer 锁，且永不在权威推进的调用链中发生。
    let next = {
        let mut binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
        let current = require_lease(&binding, lease)?;
        take_outbound(&mut binding, current)?
    };
    next.map_or(Ok(JsValue::NULL), |value| to_js(&value))
}

/// 共享 terminal 策略先于 held 的普通数据暂停；序列化仍在锁外。
fn take_outbound(
    binding: &mut Outbound,
    current: ObserverLease,
) -> Result<Option<WireOutput>, JsValue> {
    if binding.lifecycle.is_held() {
        return Ok(take_terminal(binding));
    }
    let next = match binding.pump.poll() {
        PumpRead::Ready(Read::Item {
            value:
                WorkerOutput::Update {
                    update,
                    barrier_paused,
                },
            ordinal,
        }) => {
            binding
                .pump
                .record_sent(ordinal)
                .map_err(|_| error("违反观察者发送窗口"))?;
            binding.lifecycle.sent(current, ordinal);
            Some(WireOutput::Update {
                update,
                barrier_paused,
                publication_ordinal: ordinal.to_string(),
            })
        }
        PumpRead::Ready(Read::Item {
            value: WorkerOutput::Recovery(failure),
            ordinal,
        }) => {
            binding
                .pump
                .record_sent(ordinal)
                .map_err(|_| error("违反观察者发送窗口"))?;
            binding.lifecycle.sent(current, ordinal);
            Some(WireOutput::Failure {
                failure,
                publication_ordinal: Some(ordinal.to_string()),
            })
        }
        PumpRead::Ready(Read::Empty) | PumpRead::CreditBlocked => take_terminal(binding),
        PumpRead::Ready(Read::Closed) => take_terminal(binding).or(Some(WireOutput::Closed)),
        PumpRead::Ready(Read::NeedsBaseline { through }) => {
            take_terminal(binding).or(Some(WireOutput::Gap {
                oldest_after: through.to_string(),
            }))
        }
    };
    Ok(next)
}

fn record_terminal(drive: &mut Drive, failure: HostFailure) -> Result<(), JsValue> {
    let after = drive.output.published_ordinal();
    if drive.terminal.set(Terminal { after, failure }).is_err() {
        return Err(error("市场终止记录重复"));
    }
    drive.output.signal().notify();
    Ok(())
}

fn take_terminal(binding: &mut Outbound) -> Option<WireOutput> {
    let terminal = binding.terminal.get()?;
    let missing = binding.terminal_delivery.take(
        binding.pump.terminal_readiness(terminal.after),
        binding.lifecycle.is_held(),
        binding.lifecycle.evidence().sent,
        terminal.after,
    )?;
    let mut failure = terminal.failure.clone();
    if let Some((from, through)) = missing {
        failure.message.push_str(&format!(
            "；观察流缺口：publication ordinal {from}–{through} 未送达"
        ));
    }
    Some(WireOutput::Failure {
        failure,
        publication_ordinal: None,
    })
}

#[cfg(test)]
mod terminal_tests {
    use super::*;
    use std::num::NonZeroUsize;

    fn terminal_fixture(capacity: NonZeroUsize) -> (Producer<WorkerOutput>, Outbound) {
        let (producer, observer) = publication::channel(capacity);
        (
            producer,
            Outbound {
                pump: ObserverPump::new(observer, NonZeroUsize::MIN),
                terminal: Arc::new(OnceLock::new()),
                terminal_delivery: TerminalDelivery::default(),
                lifecycle: ObserverLifecycle::default(),
                control: Arc::new(WakeEpoch::new()),
            },
        )
    }

    fn failure() -> HostFailure {
        engine::session::StepFatal::InvariantViolation {
            location: "测试终止围栏".into(),
            description: "测试首错".into(),
        }
        .into()
    }

    #[test]
    fn browser_terminal_waits_for_prefix_but_not_final_ack() {
        let (mut producer, mut binding) = terminal_fixture(NonZeroUsize::MIN);
        producer.publish(WorkerOutput::Recovery(failure()));
        assert!(binding
            .terminal
            .set(Terminal {
                after: producer.published_ordinal(),
                failure: failure(),
            })
            .is_ok());
        assert!(take_terminal(&mut binding).is_none());
        assert!(matches!(
            binding.pump.poll(),
            PumpRead::Ready(Read::Item { ordinal: 1, .. })
        ));
        binding.pump.record_sent(1).unwrap();
        assert!(matches!(binding.pump.poll(), PumpRead::CreditBlocked));
        assert!(matches!(
            take_terminal(&mut binding),
            Some(WireOutput::Failure {
                publication_ordinal: None,
                ..
            })
        ));
        assert!(take_terminal(&mut binding).is_none());
    }

    #[test]
    fn held_browser_terminal_reports_missing_prefix_once_without_releasing_hold() {
        let (mut producer, mut binding) = terminal_fixture(NonZeroUsize::new(2).unwrap());
        let lease = binding.lifecycle.hold(1);
        producer.publish(WorkerOutput::Recovery(failure()));
        assert!(binding
            .terminal
            .set(Terminal {
                after: producer.published_ordinal(),
                failure: failure()
            })
            .is_ok());
        let Some(WireOutput::Failure {
            failure,
            publication_ordinal: None,
        }) = take_outbound(&mut binding, lease).unwrap()
        else {
            panic!("held 观察者必须收到显式前缀缺口和 terminal");
        };
        assert!(failure.message.contains("publication ordinal 1–1 未送达"));
        assert!(binding.lifecycle.is_held());
        assert!(take_outbound(&mut binding, lease).unwrap().is_none());
    }

    #[test]
    fn held_browser_queue_full_terminal_does_not_require_baseline_resume() {
        let (mut producer, mut binding) = terminal_fixture(NonZeroUsize::MIN);
        let lease = binding.lifecycle.hold(1);
        producer.publish(WorkerOutput::Recovery(failure()));
        assert!(matches!(
            producer.publish(WorkerOutput::Recovery(failure())),
            publication::Push::Gap { .. }
        ));
        assert!(binding
            .terminal
            .set(Terminal {
                after: producer.published_ordinal(),
                failure: failure()
            })
            .is_ok());
        let Some(WireOutput::Failure {
            failure,
            publication_ordinal: None,
        }) = take_outbound(&mut binding, lease).unwrap()
        else {
            panic!("队满仍须terminal");
        };
        assert!(failure.message.contains("publication ordinal 1–2 未送达"));
        assert!(binding.lifecycle.is_held());
        assert!(take_outbound(&mut binding, lease).unwrap().is_none());
    }

    #[test]
    fn browser_terminal_exposes_gap_without_waiting_for_invalid_baseline() {
        let (mut producer, mut binding) = terminal_fixture(NonZeroUsize::MIN);
        producer.publish(WorkerOutput::Recovery(failure()));
        assert_eq!(
            producer.publish(WorkerOutput::Recovery(failure())),
            publication::Push::Gap { ordinal: 2 }
        );
        assert!(binding
            .terminal
            .set(Terminal {
                after: producer.published_ordinal(),
                failure: failure(),
            })
            .is_ok());
        let Some(WireOutput::Failure {
            failure,
            publication_ordinal: None,
        }) = take_terminal(&mut binding)
        else {
            panic!("前缀已有缺口时必须发布带遗漏范围的终止通知");
        };
        assert!(failure.message.contains("publication ordinal 1–2 未送达"));
    }
}

#[wasm_bindgen]
pub fn market_publisher_resume(token: u32, lease: u64, ordinal: u64) -> Result<bool, JsValue> {
    let binding = outbound(token)?;
    let mut binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
    let current = require_lease(&binding, lease)?;
    match binding.pump.resume_after(ordinal) {
        Ok(()) => Ok(binding.lifecycle.fence(current, ordinal)),
        Err(ResumeError::Stale { .. }) => Ok(false),
        Err(ResumeError::Future { .. }) => Err(error("发布基线的 ordinal 晚于已交接内容")),
    }
}

#[wasm_bindgen]
pub fn market_publisher_flushed(token: u32, lease: u64, ordinal: u64) -> Result<(), JsValue> {
    let binding = outbound(token)?;
    let mut binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
    let current = require_lease(&binding, lease)?;
    if !binding.lifecycle.flushed(current, ordinal) {
        return Err(error("发布传输确认超出已发送范围"));
    }
    Ok(())
}

#[wasm_bindgen]
pub fn market_publisher_ack(token: u32, lease: u64, ordinal: u64) -> Result<(), JsValue> {
    let binding = outbound(token)?;
    let mut binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
    let current = require_lease(&binding, lease)?;
    if binding
        .pump
        .acknowledge(ordinal)
        .map_err(|_| error("发布确认序号不是最早未确认项"))?
    {
        binding.lifecycle.accepted(current, ordinal);
        binding.pump.signal().notify();
    }
    Ok(())
}

/// 此调用必须由发布 Worker 在所有 waitAsync 返回后执行，不能留下悬空地址。
#[wasm_bindgen]
pub fn market_publisher_release(token: u32, lease: u64) -> Result<(), JsValue> {
    let mut registry = OUTBOUND
        .lock()
        .map_err(|_| error("发布端 registry 已中毒"))?;
    {
        let binding = registry
            .as_ref()
            .and_then(|all| all.get(&token))
            .ok_or_else(|| error("未知或已释放的发布令牌"))?;
        let binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
        require_lease(&binding, lease)?;
    }
    if registry
        .as_mut()
        .and_then(|all| all.remove(&token))
        .is_none()
    {
        return Err(error("未知或已释放的发布令牌"));
    }
    Ok(())
}

impl AuthorityPort for DriverPort<'_> {
    fn authority(&mut self) -> &mut Pacing {
        &mut self.drive.pacing
    }
    fn archive_blocked(&self) -> bool {
        self.drive
            .archive
            .as_ref()
            .is_some_and(|archive| archive.is_blocked())
    }
}

/// 业务写入只使用 authority 门禁，查询与替换不经过此入口。
pub(super) fn require_write(handle: u32) -> Result<(), JsValue> {
    DRIVES.with(|drives| {
        let drives = drives.borrow();
        if let Some(drive) = drives.get(&handle) {
            drive
                .pacing
                .require_control(
                    drive
                        .archive
                        .as_ref()
                        .and_then(|archive| archive.blocked_reason()),
                )
                .map_err(|source| error(&source.to_string()))?;
        }
        Ok(())
    })
}

/// 验收读取实际 Drive、ArchivePort 和 Outbound，不生成成功回执。
#[cfg(feature = "verification-harness")]
#[wasm_bindgen]
pub fn market_runner_contract_snapshot(handle: u32, id: String) -> Result<JsValue, JsValue> {
    REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let session = registry.sessions.get(&handle).ok_or_else(||error("未知验收会话"))?;
        with_drive(handle, |drive| {
            let archive = drive.archive.as_ref().ok_or_else(||error("验收需要真实归档连接"))?.trace();
            let observer = inspect_observer(handle)?;
            let lease = observer.lease.ok_or_else(||error("验收缺少实际observer lease"))?;
            let key = |key: engine::session::checkpoint::CheckpointKey| serde_json::json!({"generation":key.generation.to_string(),"writerEpoch":key.writer_epoch.to_string(),"seq":key.seq.to_string()});
            to_js(&serde_json::json!({"id":id,"generation":drive.generation.to_string(),"day":session.game().day(),"tick":session.tick().to_string(),"seq":session.seq().to_string(),"paused":!drive.pacing.is_running(),"terminal":drive.pacing.fatal_failure().map(|failure|failure.code),"civilReady":session.civil_day_ready().map_err(|source|error(&source.to_string()))?,"archive":{"queued":archive.queued.into_iter().map(key).collect::<Vec<_>>(),"durable":archive.durable.map(key),"blocked":archive.blocked},"observer":{"generation":lease.generation.to_string(),"lease":lease.lease.to_string(),"sent":observer.delivery.sent.to_string(),"flushed":observer.delivery.flushed.to_string(),"accepted":observer.delivery.accepted.to_string()}}))
        })
    })
}

/// 故障仅进入真实 authority 错误入口，成功前缀由正常命令先提交。
#[cfg(feature = "verification-harness")]
#[wasm_bindgen]
pub fn market_runner_contract_fail(handle: u32) -> Result<(), JsValue> {
    REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let session = registry
            .sessions
            .get(&handle)
            .ok_or_else(|| error("未知验收会话"))?;
        report_operation_failure(
            handle,
            session,
            HostFailure::step(engine::session::StepFatal::InvariantViolation {
                location: "authority.contract".into(),
                description: "验收注入前缀后故障".into(),
            }),
        )
        .map(|_| ())
    })
}

#[cfg(test)]
mod command_failure_tests {
    use super::*;

    #[test]
    fn paused_company_failure_is_immediately_latched_after_committed_publications() {
        let setup =
            crate::session_fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
        let handle = REGISTRY.with(|registry| registry.borrow_mut().create(setup, 17).unwrap());
        market_runner_enable(handle).unwrap();
        market_runner_step_once(handle).unwrap();
        assert!(require_compatibility_entry(handle).is_err());
        REGISTRY.with(|registry| {
            let registry = registry.borrow();
            let session = registry.sessions.get(&handle).unwrap();
            let failure = HostFailure::step(engine::session::StepFatal::InvariantViolation {
                location: "company.command.fixture".into(),
                description: "不得公开的细节".into(),
            });
            assert!(report_operation_failure(handle, session, failure).unwrap());
            DRIVES.with(|drives| {
                let drives = drives.borrow();
                let drive = drives.get(&handle).unwrap();
                assert!(!drive.pacing.is_running());
                assert_eq!(drive.pacing.fatal_failure().unwrap().code, "STEP_FATAL");
                let terminal = drive.terminal.get().unwrap();
                assert!(terminal.after > 0, "先前成功前缀必须已有发布序号");
                assert_eq!(terminal.after, drive.output.published_ordinal());
                assert_eq!(terminal.failure.context.generation.as_deref(), Some("1"));
            });
            assert!(session
                .shared_ingress()
                .enqueue_player_intent(
                    engine::AccountId(0),
                    engine::Intent::Cancel {
                        code: engine::StockId(0),
                        id: engine::OrderId(1)
                    }
                )
                .is_err());
        });
        REGISTRY.with(|registry| registry.borrow_mut().remove(handle).unwrap());
        OUTBOUND.lock().unwrap().as_mut().unwrap().remove(&handle);
    }
}

/// 公司写入可能在暂停期间失败，必须立即锁存，不等候下一次市场 turn。
pub(super) fn report_operation_failure(
    handle: u32,
    session: &ProtocolSession,
    failure: HostFailure,
) -> Result<bool, JsValue> {
    DRIVES.with(|drives| {
        let mut drives = drives.borrow_mut();
        let Some(drive) = drives.get_mut(&handle) else {
            return Ok(false);
        };
        publish_authority_failure(session, drive, failure)?;
        Ok(true)
    })
}

/// 原始 ABI 仅供没有产品 authority 的兼容或验证 owner 使用。
pub(super) fn require_compatibility_entry(handle: u32) -> Result<(), &'static str> {
    DRIVES.with(|drives| {
        if drives.borrow().contains_key(&handle) {
            Err("活动市场必须通过共享 authority 命令入口推进，不能调用原始兼容 ABI")
        } else {
            Ok(())
        }
    })
}

#[cfg(feature = "verification-harness")]
#[wasm_bindgen]
pub fn market_runner_contract_setup() -> Result<JsValue, JsValue> {
    to_js(&engine::session::authority_contract::setup())
}

/// 仅验证构建允许在实际 ArchivePort 下一次入队处注入拒绝。
#[cfg(feature = "verification-harness")]
#[wasm_bindgen]
pub fn market_runner_contract_queue_full(handle: u32) -> Result<(), JsValue> {
    with_drive(handle, |drive| {
        drive
            .archive
            .as_mut()
            .ok_or_else(|| error("验收缺少归档连接"))?
            .reject_next_enqueue();
        Ok(())
    })
}
