//! engine → WASM 绑定（ADR-0007 §4）。
//!
//! 多核：wasm-bindgen-rayon 初始化线程池后，engine 内的 rayon par_iter
//! 自动用满浏览器所有核心（SharedArrayBuffer + Atomics）。
//!
//! GameSession 不可序列化（含 `Box<dyn Strategy + Send + Sync>`），故存于 thread_local
//! 句柄注册表；仅 `Snapshot`/`Event`/`Intent`/`SessionSetup` 经 serde-wasm-bindgen 跨界。
//! JS 持 u32 句柄调 create/step/snapshot/enqueue/drop。
//!
//! 纯前端单机：player 固定 AccountId(0)（enqueue 不带 player_id）。

use engine::company::{PublicReportPage, PublicReportQuery, PublicReportSummary};
use engine::session::protocol::{EngineUpdate, ProtocolSession};
use engine::{AccountId, Intent, SaveSlot, SessionError, SessionSetup};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use wasm_bindgen::prelude::*;

/// 初始化 WASM 多线程（wasm-bindgen-rayon）。
/// 必须在 create_session 前调用。浏览器需启用 SharedArrayBuffer（COOP/COEP 头）。
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn init_threads(cores: u32) {
    let _ = wasm_bindgen_rayon::init_thread_pool(cores as usize);
}

/// 序列化为 JsValue。map 默认序列化为 JS Map（AccountId 是数字键，无法作 Object 键）；
/// 前端 host 适配器负责把 Map 规整为普通对象（Object.fromEntries）供 React/RTK 消费。
fn to_js<T: Serialize>(v: &T) -> Result<JsValue, JsValue> {
    v.serialize(&serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true))
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

fn public_dto_to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    value
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_missing_as_null(true))
        .map_err(|error| JsValue::from_str(&error.to_string()))
}

fn save_to_js<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    to_js(value)
}

thread_local! {
    static REGISTRY: RefCell<HashMap<u32, ProtocolSession>> = RefCell::new(HashMap::new());
}

#[cfg(test)]
mod protocol_tests;
static NEXT: AtomicU32 = AtomicU32::new(1);

/// Fatal engine failure delivered across the WASM boundary.
///
/// This deliberately mirrors ADR-0010's host-level contract instead of exposing a
/// Rust display string that JavaScript would have to classify heuristically.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostFailure {
    pub code: &'static str,
    pub message: String,
}

impl From<engine::session::StepFatal> for HostFailure {
    fn from(error: engine::session::StepFatal) -> Self {
        Self {
            code: "STEP_FATAL",
            message: error.to_string(),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum StepUpdateError {
    Operation(String),
    Fatal(HostFailure),
}

impl std::fmt::Display for StepUpdateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Operation(message) => formatter.write_str(message),
            Self::Fatal(failure) => formatter.write_str(&failure.message),
        }
    }
}

fn step_update_error_to_js(error: StepUpdateError) -> JsValue {
    match error {
        StepUpdateError::Operation(message) => JsValue::from_str(&message),
        StepUpdateError::Fatal(failure) => to_js(&failure).unwrap_or_else(|serialize_error| {
            let serialize_error = serialize_error
                .as_string()
                .unwrap_or_else(|| format!("{serialize_error:?}"));
            JsValue::from_str(&format!(
                "failed to serialize HostFailure {}: {}; original failure: {}",
                failure.code, serialize_error, failure.message
            ))
        }),
    }
}

fn session_error_to_js(error: SessionError) -> JsValue {
    match error {
        SessionError::Step(fatal) => step_update_error_to_js(StepUpdateError::Fatal(fatal.into())),
        other => JsValue::from_str(&other.to_string()),
    }
}

/// 创建会话。setup 为 SessionSetup 的 JS 对象，seed 为种子。
/// 返回句柄 u32。失败（setup 非法）→ 抛 JsValue。
#[wasm_bindgen]
pub fn create_session(setup: JsValue, seed: u64) -> Result<u32, JsValue> {
    let setup: SessionSetup = serde_wasm_bindgen::from_value(setup)?;
    let sess = ProtocolSession::new(setup, seed).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    REGISTRY.with(|r| r.borrow_mut().insert(id, sess));
    Ok(id)
}

/// Advances one committed tick and returns a validated EngineUpdate.
#[wasm_bindgen]
pub fn step(handle: u32) -> Result<JsValue, JsValue> {
    let update = step_update(handle).map_err(step_update_error_to_js)?;
    to_js(&update)
}

fn step_update(handle: u32) -> Result<EngineUpdate, StepUpdateError> {
    REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let session = registry.get_mut(&handle).ok_or_else(|| {
            StepUpdateError::Operation(format!("invalid session handle: {handle}"))
        })?;
        if session.civil_day_ready().map_err(|error| match error {
            SessionError::Step(fatal) => StepUpdateError::Fatal(fatal.into()),
            other => StepUpdateError::Operation(other.to_string()),
        })? {
            return Err(StepUpdateError::Operation(
                "civil day barrier must be published before stepping".into(),
            ));
        }
        let frame = session
            .step_frame()
            .map_err(|error| StepUpdateError::Fatal(error.into()))?;
        let batch = session
            .tick_batch(vec![frame])
            .map_err(|error| StepUpdateError::Fatal(error.into()))?;
        Ok(EngineUpdate::TickBatch(batch))
    })
}

/// 拉完整快照（首次连/重连/存档）。
#[wasm_bindgen]
pub fn snapshot(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |sess| to_js(&sess.snapshot()))
}

/// 高频运行快照：不复制 360 日历史，仅供日界刷新报价、昨收和账户状态。
#[wasm_bindgen]
pub fn runtime_snapshot(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |sess| to_js(&sess.runtime_snapshot()))
}

/// 当前 tick（已推进数）。
#[wasm_bindgen]
pub fn tick(handle: u32) -> Result<u64, JsValue> {
    with_session(handle, |sess| Ok(sess.tick()))
}

/// 当前交易日。
#[wasm_bindgen]
pub fn day(handle: u32) -> Result<u32, JsValue> {
    with_session(handle, |sess| Ok(sess.day()))
}

/// 当前自然日（ISO YYYY-MM-DD）。
#[wasm_bindgen]
pub fn civil_date(handle: u32) -> Result<String, JsValue> {
    with_session(handle, |sess| Ok(sess.civil_date().to_iso()))
}

/// Completes the current civil day and returns its ordered public events.
#[wasm_bindgen]
pub fn end_civil_day(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |sess| {
        let report = sess.end_civil_day_update().map_err(session_error_to_js)?;
        to_js(&EngineUpdate::CivilUpdate(Box::new(report)))
    })
}

/// 查询当前自然日已公开的公司报告页。
#[wasm_bindgen]
pub fn public_report_page(handle: u32, query: JsValue) -> Result<JsValue, JsValue> {
    let query: PublicReportQuery = serde_wasm_bindgen::from_value(query)?;
    with_session(handle, |sess| {
        let page: PublicReportPage = sess
            .query_public_reports(&query)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        public_dto_to_js(&page)
    })
}

/// 按不可变十进制发布 ID 查询当前自然日可见的公开报告。
#[wasm_bindgen]
pub fn public_report_by_id(handle: u32, id: String) -> Result<JsValue, JsValue> {
    with_session(handle, |sess| {
        let report: PublicReportSummary = sess
            .public_report_by_id(id)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        public_dto_to_js(&report)
    })
}

#[cfg(feature = "simulation-diagnostics")]
#[wasm_bindgen]
pub fn npc_decision_trace(handle: u32, account: u64) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        to_js(&session.npc_decision_diagnostics(AccountId(account)))
    })
}

/// 玩家入队意图（player 固定 AccountId(0)，单机纯前端）。intent 为 Intent 的 JS 对象。
#[wasm_bindgen]
pub fn enqueue(handle: u32, intent: JsValue) -> Result<(), JsValue> {
    let intent: Intent = serde_wasm_bindgen::from_value(intent)?;
    with_session(handle, |sess| {
        sess.enqueue_player_intent(AccountId(0), intent)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    })
}

/// 销毁会话（释放内存）。
#[wasm_bindgen]
pub fn drop_session(handle: u32) {
    REGISTRY.with(|r| {
        r.borrow_mut().remove(&handle);
    });
}

/// 生成存档（精确到交易日）。返回 SaveSlot 的 JS 对象。
#[wasm_bindgen]
pub fn save(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |sess| {
        let slot = sess
            .save()
            .map_err(|error| step_update_error_to_js(StepUpdateError::Fatal(error.into())))?;
        save_to_js(&slot)
    })
}

/// 从存档恢复（精确到天）。返回新句柄。
#[wasm_bindgen]
pub fn restore(save_slot: JsValue) -> Result<u32, JsValue> {
    let slot: SaveSlot = serde_wasm_bindgen::from_value(save_slot)?;
    let sess = ProtocolSession::restore(&slot).map_err(|e| JsValue::from_str(&e.to_string()))?;
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    REGISTRY.with(|r| r.borrow_mut().insert(id, sess));
    Ok(id)
}

#[wasm_bindgen]
pub fn restore_json(save_json: String) -> Result<u32, JsValue> {
    let slot: SaveSlot =
        serde_json::from_str(&save_json).map_err(|error| JsValue::from_str(&error.to_string()))?;
    let sess =
        ProtocolSession::restore(&slot).map_err(|error| JsValue::from_str(&error.to_string()))?;
    let id = NEXT.fetch_add(1, Ordering::SeqCst);
    REGISTRY.with(|registry| registry.borrow_mut().insert(id, sess));
    Ok(id)
}

/// 句柄内执行闭包；句柄无效 → 抛 JsValue。
fn with_session<T>(
    handle: u32,
    f: impl FnOnce(&mut ProtocolSession) -> Result<T, JsValue>,
) -> Result<T, JsValue> {
    REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        match reg.get_mut(&handle) {
            Some(sess) => f(sess),
            None => Err(JsValue::from_str(&format!(
                "invalid session handle: {handle}"
            ))),
        }
    })
}
