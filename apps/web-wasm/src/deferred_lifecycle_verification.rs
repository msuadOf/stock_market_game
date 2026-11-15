//! 真实 LibOS 析构消费者的测试装配。所有结果都来自产品队列与实际 owner。

use libos::deferred_drop::{
    CloseError, QUEUE_CAPACITY, RejectReason, StartError, Submission, close_wasm_reclaimer, defer,
    defer_job, degradation, start_wasm_reclaimer, try_defer, verify_close_wasm_reclaimer,
};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    sync::{Arc, Mutex, mpsc},
    thread::ThreadId,
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(1);
type Drops = Arc<Mutex<Vec<(usize, &'static str)>>>;

struct Probe {
    id: usize,
    caller: ThreadId,
    drops: Drops,
}

impl Drop for Probe {
    fn drop(&mut self) {
        let by = if std::thread::current().id() == self.caller {
            "caller"
        } else {
            "consumer"
        };
        self.drops.lock().unwrap().push((self.id, by));
    }
}

fn probe(id: usize, drops: &Drops) -> Probe {
    Probe { id, caller: std::thread::current().id(), drops: Arc::clone(drops) }
}

fn tags(drops: &Drops) -> Vec<Value> {
    drops.lock().unwrap().iter().map(|(id, by)| json!({ "id": id, "by": by })).collect()
}

fn per_job(drops: &Drops, n: usize) -> Vec<usize> {
    let mut counts = vec![0; n];
    for (id, _) in drops.lock().unwrap().iter() { counts[*id] += 1; }
    counts
}

fn disposition(result: Submission) -> &'static str {
    match result {
        Submission::Queued => "queued",
        Submission::Inline => "inline",
        Submission::FullInline => "full-inline",
        Submission::DisconnectedInline => "disconnected-inline",
        Submission::SpawnFailedInline => "spawn-failed-inline",
    }
}

fn started() -> Result<(), &'static str> {
    start_wasm_reclaimer().map_err(|_| "真实 Reclaimer 未能启动")
}

fn close() -> Result<&'static str, &'static str> {
    match close_wasm_reclaimer() {
        Ok(true) => Ok("drained-and-joined"),
        Ok(false) => Ok("not-started"),
        Err(CloseError::AlreadyClosed) => Ok("already-closed"),
        Err(CloseError::Join(_)) => Err("真实 Reclaimer join 失败"),
    }
}

fn hold_consumer(on_release: impl FnOnce() + Send + 'static) -> Result<mpsc::Sender<()>, &'static str> {
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let result = defer_job(Box::new(move || {
        if entered_tx.send(()).is_ok() && release_rx.recv().is_ok() {
            on_release();
        }
    }));
    if result != Submission::Queued { return Err("消费者栏杆必须真实排队"); }
    libos::wait::recv_deadline(&entered_rx, WAIT, false)
        .map_err(|_| "未收到真实析构消费者进入栏杆的回执")?;
    Ok(release_tx)
}

pub(super) fn observe_normal() -> Result<Value, &'static str> {
    started()?;
    let release = hold_consumer(|| {})?;
    let drops = Drops::default();
    let submissions = [defer(probe(0, &drops)), defer(probe(1, &drops))].map(disposition);
    let on_return = per_job(&drops, 2);
    release.send(()).map_err(|_| "析构栏杆在释放前断开")?;
    if close()? != "drained-and-joined" { return Err("正常析构必须获得 Rust join 证明"); }
    Ok(json!({
        "disposition": submissions,
        "completedOnReturn": on_return,
        "completedAtBoundary": per_job(&drops, 2),
    }))
}

fn saturation() -> Result<Value, &'static str> {
    started()?;
    let release = hold_consumer(|| {})?;
    let drops = Drops::default();
    let submissions: Vec<_> = (0..=QUEUE_CAPACITY)
        .map(|id| disposition(defer(probe(id, &drops))))
        .collect();
    let gated = tags(&drops);
    let pressure_diagnostic = degradation().map(|value| format!("{value:?}"));
    release.send(()).map_err(|_| "析构栏杆在释放前断开")?;
    Ok(json!({
        "gateEntered": true,
        "capacity": QUEUE_CAPACITY,
        "submissions": submissions,
        "completedWhileGated": gated,
        "degradationAfterOverflow": pressure_diagnostic,
        "close": close()?,
        "completedAtClose": tags(&drops),
    }))
}

fn rejection_witness() -> Value {
    let drops = Drops::default();
    let value = Box::new(probe(0, &drops));
    let identity = (&*value as *const Probe).addr();
    match try_defer(value) {
        Err((reason, returned)) => {
            let same = (&*returned as *const Probe).addr() == identity;
            let before = per_job(&drops, 1)[0];
            drop(returned);
            let after = per_job(&drops, 1)[0];
            let code = match reason {
                RejectReason::NotStarted => "not-started",
                RejectReason::StartFailed => "start-failed",
                RejectReason::Closed => "closed",
            };
            json!({"result": "rejected", "code": code, "sameValueReturned": same, "dropsAtReturn": before, "dropsAfterCallerDrop": after})
        }
        Ok(result) => json!({"result": disposition(result), "dropsAtReturn": per_job(&drops, 1)[0]}),
    }
}

fn close_lifecycle() -> Result<Value, &'static str> {
    let before = rejection_witness();
    let close_before_start = close()?;
    started()?;
    let duplicate = match start_wasm_reclaimer() {
        Err(StartError::AlreadyStarted) => "already-started",
        Err(_) => "different-start-error",
        Ok(()) => "duplicate-started",
    };
    let order = Arc::new(Mutex::new(Vec::new()));
    let release = hold_consumer(|| {})?;
    let drops = Drops::default();
    let submissions = [defer(probe(0, &drops)), defer(probe(1, &drops))].map(disposition);
    let witness_order = Arc::clone(&order);
    let marker = defer_job(Box::new(move || witness_order.lock().unwrap().push("consumer-completed")));
    if marker != Submission::Queued { return Err("关闭证明的完成 marker 必须真实排队"); }
    let mut at_entry = None;
    let mut release_result = None;
    let closed = verify_close_wasm_reclaimer(|| {
        order.lock().unwrap().push("close-entered");
        at_entry = Some(tags(&drops));
        order.lock().unwrap().push("gate-released");
        release_result = Some(release.send(()));
    }).map_err(|_| "真实 Reclaimer join 失败")?;
    release_result.ok_or("没有进入真实 close")?.map_err(|_| "析构栏杆在释放前断开")?;
    order.lock().unwrap().push("close-returned");
    let observed_order = order.lock().unwrap().clone();
    Ok(json!({
        "submitBeforeStart": before, "closeBeforeStart": close_before_start, "start": "started",
        "duplicateStart": duplicate, "submissions": submissions,
        "completedAtCloseEntry": at_entry, "order": observed_order,
        "completedAtClose": tags(&drops),
        "close": if closed { "drained-and-joined" } else { "not-started" },
        "duplicateClose": close()?, "submitAfterClose": rejection_witness(),
    }))
}

thread_local! {
    static PANIC_GATE: RefCell<Option<mpsc::Sender<()>>> = const { RefCell::new(None) };
}

fn arm_consumer_panic() -> Result<Value, &'static str> {
    started()?;
    let gate = hold_consumer(|| panic!("LIBOS_RECLAIMER_PANIC_PROBE"))?;
    PANIC_GATE.with(|slot| *slot.borrow_mut() = Some(gate));
    Ok(json!({"submission": "queued", "workerEntered": true, "releasedByCaller": false}))
}

fn release_consumer_panic() -> Result<Value, &'static str> {
    PANIC_GATE.with(|slot| slot.borrow_mut().take()).ok_or("缺少真实消费者栏杆")?
        .send(()).map_err(|_| "真实消费者栏杆已断开")?;
    Ok(Value::Null)
}

fn spawn_failure() -> Result<Value, &'static str> {
    let denied = match start_wasm_reclaimer() {
        Err(StartError::SpawnFailed(error)) => error.to_string(),
        Err(_) => return Err("非真实 spawn 失败不能覆盖本用例"),
        Ok(()) => {
            close()?;
            return Err("意外启动了 supervisor 应拒绝的析构 Worker");
        }
    };
    Ok(json!({
        "start": "spawn-failed", "detail": denied,
        "close": close()?, "submitAfterFailedStart": rejection_witness(),
    }))
}

pub(super) fn observe(stage: &str) -> Result<Value, &'static str> {
    match stage {
        "saturation" => saturation(),
        "close" => close_lifecycle(),
        "arm-consumer-panic" => arm_consumer_panic(),
        "release-consumer-panic" => release_consumer_panic(),
        "spawn-failure" => spawn_failure(),
        _ => Err("未知 LibOS Reclaimer 验证阶段"),
    }
}
