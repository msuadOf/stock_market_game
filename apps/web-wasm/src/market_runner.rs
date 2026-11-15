//! 浏览器会话 ABI：权威端只向 Rust 邮箱交接；发布 Worker 独占出队与序列化。
use super::{to_js, HostFailure, REGISTRY};
use engine::session::{
    protocol::{CivilUpdate, EngineUpdate, PausePreferences, ProtocolSession, TickBatch},
    publication::{self, ObserverPump, Producer, PumpRead, Read, ResumeError, TerminalReadiness},
    runner::{self, CycleError, DrivePort, Pacing, WakePlan},
};
use libos::{clock::Instant, wake_epoch::WakeEpoch};
use serde::Serialize;
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock},
};
use wasm_bindgen::prelude::*;

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
    terminal_delivered: bool,
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
        let tick = registry
            .borrow()
            .sessions
            .get(&handle)
            .ok_or_else(|| error("未知的市场推进会话"))?
            .tick();
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
                    terminal_delivered: false,
                    control: Arc::clone(&control),
                })),
            );
            drives.insert(
                handle,
                Drive {
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
        drop(drive.archive.take());
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
pub fn market_archive_receipt(handle: u32, generation: u64, sequence: u64) -> Result<(), JsValue> {
    with_drive(handle, |drive| {
        if let Some(archive) = &mut drive.archive {
            if archive.generation == generation {
                archive.receive(sequence)?;
            }
        }
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
}

#[wasm_bindgen]
pub fn market_runner_observer_baseline(handle: u32) -> Result<JsValue, JsValue> {
    REGISTRY.with(|registry| {
        let registry = registry.borrow();
        let session = registry
            .sessions
            .get(&handle)
            .ok_or_else(|| error("未知的市场推进会话"))?;
        let (snapshot, working_orders) = session.observer_baseline();
        let publication_ordinal = market_runner_publication_ordinal(handle)?;
        to_js(&WorkerBaseline {
            snapshot,
            working_orders,
            publication_ordinal,
            latest_checkpoint: session.latest_day_end_key(),
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
        if running && drive.terminal.get().is_some() {
            return Err(error("市场已因致命错误停止，需恢复或启动新局"));
        }
        drive.pacing.set_running(running, tick, Instant::now());
        Ok(())
    })
}

#[wasm_bindgen]
pub fn market_runner_set_speed(handle: u32, multiplier: f64) -> Result<(), JsValue> {
    let tick = session_tick(handle)?;
    with_drive(handle, |drive| {
        drive
            .pacing
            .apply_speed(multiplier, tick, Instant::now())
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
        drive.pause = PausePreferences {
            pause_after_close: after_close,
            pause_before_open: before_open,
        };
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

fn archive_failure(message: String) -> HostFailure {
    HostFailure {
        code: "DAY_END_ARCHIVE_FAILED",
        message,
        r#where: "web-wasm.market_runner.checkpoint".into(),
        cause: None,
        context: super::FailureContext {
            operation: "day_end_archive",
            fault: None,
            tick: None,
            seq: None,
            day: None,
            generation: None,
        },
        recoverable: true,
        recovery_actions: vec!["检查浏览器存档权限和空间", "保留上一份有效存档"],
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct TurnResult {
    delay_ms: Option<f64>,
    running: bool,
    committed_ticks: u64,
}

fn civil_failure(source: engine::session::SessionError, recoverable: bool) -> HostFailure {
    let mut failure = HostFailure::civil(source);
    if recoverable {
        failure.code = "REPORT_CORRECTION_PREPARE_REJECTED";
        failure.r#where = "web-wasm.market_runner.end_civil".into();
        failure.recoverable = true;
        failure.recovery_actions = vec!["修正日结准备错误后重试日结"];
    }
    failure
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
                    let result =
                        runner::run_cycle(&mut DriverPort { session, drive }, permit, pause);
                    if let Err(failure) = runner::apply_cycle_pacing(
                        &mut drive.pacing,
                        session.tick(),
                        Instant::now(),
                        result,
                    ) {
                        let (mut error, range) = match failure {
                            CycleError::Advance {
                                source,
                                unpublished,
                            } => (HostFailure::civil(source), unpublished),
                            CycleError::Batch {
                                source,
                                unpublished,
                            } => (source.into(), Some(unpublished)),
                            CycleError::Civil { source, retryable } => {
                                (civil_failure(source.into_error(), retryable), None)
                            }
                            CycleError::Stage(message) => (archive_failure(message), None),
                        };
                        if let Some(range) = range {
                            range.describe(&mut error.message);
                        }
                        // 终止错误与成功前缀共享发布者，不能从控制端抢跑。
                        let mut error = error.at_session(session);
                        runner::apply_authority_failure(
                            session,
                            error.recoverable,
                            &mut error.message,
                        );
                        if error.recoverable {
                            drive.output.publish(WorkerOutput::Recovery(error));
                        } else {
                            record_terminal(drive, error)?;
                        }
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
    use runner::manual::{manual_tick, ManualStepError};

    with_manual_authority(handle, |session, drive| {
        match manual_tick(session, drive.pacing.is_running()) {
            Ok(batch) => {
                stage_update(drive, EngineUpdate::TickBatch(Box::new(batch)), false);
                Ok(())
            }
            Err(ManualStepError::Step(source)) => {
                let failure = HostFailure::from(source).at_session(session);
                publish_manual_failure(session, drive, failure)
            }
            Err(source) => Err(error(&source.to_string())),
        }
    })
}

#[wasm_bindgen]
pub fn market_runner_end_civil(handle: u32) -> Result<(), JsValue> {
    use runner::manual::{manual_civil, ManualCivilError};

    with_manual_authority(handle, |session, drive| {
        match manual_civil(session, drive.pause) {
            Ok((civil, paused)) => {
                if paused {
                    drive
                        .pacing
                        .pause_at_civil_boundary(session.tick(), Instant::now());
                }
                match stage_civil_checkpoint(session, drive, civil, paused) {
                    Ok(()) => Ok(()),
                    Err(message) => publish_manual_failure(
                        session,
                        drive,
                        archive_failure(message).at_session(session),
                    ),
                }
            }
            Err(ManualCivilError::Rejected(source)) => Err(error(&source.to_string())),
            Err(ManualCivilError::Retryable(source)) => publish_manual_failure(
                session,
                drive,
                civil_failure(source, true).at_session(session),
            ),
            Err(ManualCivilError::Fatal(source)) => publish_manual_failure(
                session,
                drive,
                civil_failure(source, false).at_session(session),
            ),
        }
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
        with_drive(handle, |drive| {
            if drive.terminal.get().is_some() {
                return Err(error("市场权威已经终止；请重新打开有效日终档或新局"));
            }
            operation(session, drive)
        })
    })
}

fn publish_manual_failure(
    session: &ProtocolSession,
    drive: &mut Drive,
    mut failure: HostFailure,
) -> Result<(), JsValue> {
    drive.pacing.stop_after_failure(Instant::now());
    runner::apply_authority_failure(session, failure.recoverable, &mut failure.message);
    let message = failure.message.clone();
    if failure.recoverable {
        drive.output.publish(WorkerOutput::Recovery(failure));
    } else {
        record_terminal(drive, failure)?;
    }
    // 仅在控制通道结束请求；结构化权威失败走发布通道，不抢跑已提交前缀。
    Err(error(&message))
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

#[wasm_bindgen]
pub fn market_publisher_take(token: u32) -> Result<JsValue, JsValue> {
    let binding = outbound(token)?;
    // 序列化不持有 registry/observer 锁，且永不在权威推进的调用链中发生。
    let next = {
        let mut binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
        match binding.pump.poll() {
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
                Some(WireOutput::Failure {
                    failure,
                    publication_ordinal: Some(ordinal.to_string()),
                })
            }
            PumpRead::Ready(Read::Empty) | PumpRead::CreditBlocked => take_terminal(&mut binding),
            PumpRead::Ready(Read::Closed) => {
                take_terminal(&mut binding).or(Some(WireOutput::Closed))
            }
            PumpRead::Ready(Read::NeedsBaseline { through }) => {
                take_terminal(&mut binding).or(Some(WireOutput::Gap {
                    oldest_after: through.to_string(),
                }))
            }
        }
    };
    next.map_or(Ok(JsValue::NULL), |value| to_js(&value))
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
    if binding.terminal_delivered {
        return None;
    }
    let terminal = binding.terminal.get()?;
    let failure = match binding.pump.terminal_readiness(terminal.after) {
        TerminalReadiness::Pending => return None,
        TerminalReadiness::Ready => terminal.failure.clone(),
        TerminalReadiness::Missing { from, through } => {
            let mut failure = terminal.failure.clone();
            failure.message.push_str(&format!(
                "；观察流缺口：publication ordinal {from}–{through} 未送达"
            ));
            failure
        }
    };
    binding.terminal_delivered = true;
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
                terminal_delivered: false,
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
pub fn market_publisher_resume(token: u32, ordinal: u64) -> Result<bool, JsValue> {
    let binding = outbound(token)?;
    let mut binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
    match binding.pump.resume_after(ordinal) {
        Ok(()) => Ok(true),
        Err(ResumeError::Stale { .. }) => Ok(false),
        Err(ResumeError::Future { .. }) => Err(error("发布基线的 ordinal 晚于已交接内容")),
    }
}

#[wasm_bindgen]
pub fn market_publisher_ack(token: u32, ordinal: u64) -> Result<(), JsValue> {
    let binding = outbound(token)?;
    let mut binding = binding.lock().map_err(|_| error("发布端绑定已中毒"))?;
    if binding
        .pump
        .acknowledge(ordinal)
        .map_err(|_| error("发布确认序号不是最早未确认项"))?
    {
        binding.pump.signal().notify();
    }
    Ok(())
}

/// 此调用必须由发布 Worker 在所有 waitAsync 返回后执行，不能留下悬空地址。
#[wasm_bindgen]
pub fn market_publisher_release(token: u32) -> Result<(), JsValue> {
    let mut registry = OUTBOUND
        .lock()
        .map_err(|_| error("发布端 registry 已中毒"))?;
    if registry
        .as_mut()
        .and_then(|all| all.remove(&token))
        .is_none()
    {
        return Err(error("未知或已释放的发布令牌"));
    }
    Ok(())
}
