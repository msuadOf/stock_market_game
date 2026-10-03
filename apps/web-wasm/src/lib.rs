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
use engine::session::protocol::{EngineUpdate, ProtocolSession, SaveCandidateKey};
use engine::{AccountId, Intent, SaveSlot, SessionError, SessionSetup};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use wasm_bindgen::prelude::*;

// Export the Rayon bootstrap itself; the Web Worker awaits its Promise before
// constructing a session, so initialization errors cannot be silently lost.
#[cfg(target_arch = "wasm32")]
pub use wasm_bindgen_rayon::init_thread_pool;

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
    static REGISTRY: RefCell<SessionRegistry> = RefCell::new(SessionRegistry::default());
}

#[cfg(test)]
mod protocol_tests;
static NEXT: AtomicU32 = AtomicU32::new(1);

/// WASM Worker 局部的句柄 owner；ProtocolSession 仍拥有引擎状态。
#[derive(Default)]
struct SessionRegistry {
    sessions: HashMap<u32, ProtocolSession>,
}

impl SessionRegistry {
    fn register(&mut self, session: ProtocolSession) -> u32 {
        let id = NEXT.fetch_add(1, Ordering::SeqCst);
        self.sessions.insert(id, session);
        id
    }

    fn create(&mut self, setup: SessionSetup, seed: u64) -> Result<u32, SessionError> {
        let session = ProtocolSession::new(setup, seed)?;
        Ok(self.register(session))
    }

    fn restore(&mut self, slot: &SaveSlot) -> Result<u32, SessionError> {
        let session = ProtocolSession::restore(slot)?;
        Ok(self.register(session))
    }

    fn with_session<T>(
        &mut self,
        handle: u32,
        operation: impl FnOnce(&mut ProtocolSession) -> T,
    ) -> Result<T, String> {
        self.sessions
            .get_mut(&handle)
            .map(operation)
            .ok_or_else(|| format!("invalid session handle: {handle}"))
    }

    fn remove(&mut self, handle: u32) {
        self.sessions.remove(&handle);
    }

    fn step_update(&mut self, handle: u32) -> Result<EngineUpdate, StepUpdateError> {
        let session = self.sessions.get_mut(&handle).ok_or_else(|| {
            StepUpdateError::Operation(format!("invalid session handle: {handle}"))
        })?;
        let result = (|| {
            if session
                .civil_day_ready()
                .map_err(civil_error_to_step_update_error)?
            {
                let civil = session
                    .end_civil_day_update()
                    .map_err(civil_error_to_step_update_error)?;
                return Ok(EngineUpdate::CivilUpdate(Box::new(civil)));
            }
            let frame = session
                .step_frame()
                .map_err(|error| StepUpdateError::Fatal(Box::new(error.into())))?;
            let batch = session
                .tick_batch(vec![frame])
                .map_err(|error| StepUpdateError::Fatal(Box::new(error.into())))?;
            Ok(EngineUpdate::TickBatch(Box::new(batch)))
        })();
        result.map_err(|error| match error {
            StepUpdateError::Fatal(failure) => {
                StepUpdateError::Fatal(Box::new((*failure).at_session(session)))
            }
            other => other,
        })
    }
}

/// Fatal engine failure delivered across the WASM boundary.
///
/// This deliberately mirrors ADR-0010's host-level contract instead of exposing a
/// Rust display string that JavaScript would have to classify heuristically.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct HostFailure {
    pub code: &'static str,
    pub message: String,
    pub r#where: String,
    pub cause: Option<Box<FailureCause>>,
    pub context: FailureContext,
    pub recoverable: bool,
    #[serde(rename = "recoveryActions")]
    pub recovery_actions: Vec<&'static str>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FailureContext {
    pub operation: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tick: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub day: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generation: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct FailureCause {
    code: &'static str,
    message: String,
    cause: Option<Box<FailureCause>>,
}

fn failure_description(error: &(dyn std::error::Error + 'static)) -> (&'static str, &'static str) {
    if let Some(error) = error.downcast_ref::<SessionError>() {
        return match error {
            SessionError::Closing(_) => (
                "SESSION_CLOSING_FAILED",
                "自然日日终封账失败（私有详情已脱敏）",
            ),
            SessionError::InvalidSave(_) => (
                "SESSION_STATE_INVALID",
                "日终存档或协议状态校验失败（原始详情已脱敏）",
            ),
            SessionError::Step(_) => (
                "INVARIANT_VIOLATION",
                "引擎不变量校验失败（原始详情已脱敏）",
            ),
            _ => ("SESSION_OPERATION_FAILED", "会话操作失败（原始详情已脱敏）"),
        };
    }
    if let Some(error) = error.downcast_ref::<engine::accounting::closing::ClosingError>() {
        return match error {
            engine::accounting::closing::ClosingError::Accounting(_) => (
                "CLOSING_ACCOUNTING_FAILED",
                "日终封账的会计处理失败（私有详情已脱敏）",
            ),
            engine::accounting::closing::ClosingError::Report(_) => (
                "CLOSING_REPORT_FAILED",
                "日终封账的报表处理失败（私有详情已脱敏）",
            ),
            _ => (
                "CLOSING_VALIDATION_FAILED",
                "日终封账校验失败（私有详情已脱敏）",
            ),
        };
    }
    if let Some(error) = error.downcast_ref::<engine::accounting::AccountingError>() {
        return match error {
            engine::accounting::AccountingError::AmountOverflow { .. } => (
                "ACCOUNTING_AMOUNT_OVERFLOW",
                "公司会计金额运算溢出（操作数已脱敏）",
            ),
            _ => (
                "ACCOUNTING_VALIDATION_FAILED",
                "公司会计校验失败（私有详情已脱敏）",
            ),
        };
    }
    if error.is::<engine::session::StepFatal>() {
        return (
            "INVARIANT_VIOLATION",
            "引擎不变量校验失败（原始详情已脱敏）",
        );
    }
    ("ERROR_DETAILS_REDACTED", "原始错误类型未识别，详情未公开")
}

fn failure_cause(error: &(dyn std::error::Error + 'static)) -> Option<Box<FailureCause>> {
    error.source().map(|source| {
        let (code, message) = failure_description(source);
        Box::new(FailureCause {
            code,
            message: message.to_owned(),
            cause: failure_cause(source),
        })
    })
}

impl FailureContext {
    fn new(operation: &'static str) -> Self {
        Self {
            operation,
            tick: None,
            seq: None,
            day: None,
            generation: None,
        }
    }
}

impl From<engine::session::StepFatal> for HostFailure {
    fn from(error: engine::session::StepFatal) -> Self {
        Self::step(error)
    }
}

impl HostFailure {
    fn at_session(mut self, session: &ProtocolSession) -> Self {
        self.context.tick = Some(session.tick());
        self.context.seq = Some(session.seq());
        self.context.day = Some(session.day());
        self
    }

    fn step(error: engine::session::StepFatal) -> Self {
        let engine::session::StepFatal::InvariantViolation { ref location, .. } = error;
        Self {
            code: "STEP_FATAL",
            message: failure_description(&error).1.to_owned(),
            r#where: location.clone(),
            cause: failure_cause(&error),
            context: FailureContext::new("step"),
            recoverable: false,
            recovery_actions: vec![
                "停止当前会话；重新打开上一份有效日终存档或新局",
                "复制脱敏错误详情反馈",
            ],
        }
    }

    fn civil(error: SessionError) -> Self {
        match error {
            SessionError::Step(fatal) => Self::step(fatal),
            other => Self {
                code: "CIVIL_DAY_SETTLEMENT_FAILED",
                message: failure_description(&other).1.to_owned(),
                r#where: "web-wasm.end_civil_day_update".into(),
                cause: failure_cause(&other),
                context: FailureContext::new("endCivilDay"),
                recoverable: false,
                recovery_actions: vec![
                    "停止当前会话；重新打开上一份有效日终存档或新局",
                    "复制脱敏错误详情反馈",
                ],
            },
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum StepUpdateError {
    Operation(String),
    Fatal(Box<HostFailure>),
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
        StepUpdateError::Fatal(failure) => to_js(&*failure).unwrap_or_else(|serialize_error| {
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

fn session_error_to_step_update_error(error: SessionError) -> StepUpdateError {
    match error {
        SessionError::Step(fatal) => StepUpdateError::Fatal(Box::new(fatal.into())),
        other => StepUpdateError::Operation(other.to_string()),
    }
}

fn session_error_to_js(error: SessionError) -> JsValue {
    step_update_error_to_js(session_error_to_step_update_error(error))
}

fn civil_error_to_step_update_error(error: SessionError) -> StepUpdateError {
    StepUpdateError::Fatal(Box::new(HostFailure::civil(error)))
}

/// 创建会话。setup 为 SessionSetup 的 JS 对象，seed 为种子。
/// 返回句柄 u32。失败（setup 非法）→ 抛 JsValue。
#[wasm_bindgen]
pub fn create_session(setup: JsValue, seed: u64) -> Result<u32, JsValue> {
    let setup: SessionSetup = serde_wasm_bindgen::from_value(setup)?;
    REGISTRY
        .with(|registry| registry.borrow_mut().create(setup, seed))
        .map_err(session_error_to_js)
}

/// Returns the next ordered protocol update.
///
/// A completed market day yields its full `CivilUpdate` barrier before another
/// tick can be committed; otherwise this advances one tick and returns a
/// validated one-frame `TickBatch`.
#[wasm_bindgen]
pub fn step(handle: u32) -> Result<JsValue, JsValue> {
    let update = step_update(handle).map_err(step_update_error_to_js)?;
    to_js(&update)
}

fn step_update(handle: u32) -> Result<EngineUpdate, StepUpdateError> {
    REGISTRY.with(|registry| registry.borrow_mut().step_update(handle))
}

/// 拉完整快照（首次连/重连/存档）。
#[wasm_bindgen]
pub fn snapshot(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |sess| to_js(&sess.snapshot()))
}

#[wasm_bindgen]
pub fn prepare_public_baseline(handle: u32) -> Result<(), JsValue> {
    with_session(handle, |sess| {
        sess.prepare_public_baseline();
        Ok(())
    })
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
        let report = sess.end_civil_day_update().map_err(|error| {
            step_update_error_to_js(StepUpdateError::Fatal(Box::new(
                HostFailure::civil(error).at_session(sess),
            )))
        })?;
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
            .map_err(session_error_to_js)?;
        public_dto_to_js(&page)
    })
}

/// 按不可变十进制发布 ID 查询当前自然日可见的公开报告。
#[wasm_bindgen]
pub fn public_report_by_id(handle: u32, id: String) -> Result<JsValue, JsValue> {
    with_session(handle, |sess| {
        let report: PublicReportSummary =
            sess.public_report_by_id(id).map_err(session_error_to_js)?;
        public_dto_to_js(&report)
    })
}

/// 查询当前玩家的实时活动委托；仅读协议会话，不生成持久存档。
#[wasm_bindgen]
pub fn player_working_orders(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        public_dto_to_js(&session.player_working_orders())
    })
}

#[wasm_bindgen]
pub fn calculate_indicators(prices: Vec<f64>, candles: JsValue) -> Result<JsValue, JsValue> {
    let candles: Vec<engine::indicators::OhlcBar> = serde_wasm_bindgen::from_value(candles)?;
    let indicators = engine::indicators::calculate_indicators(&prices, &candles)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    to_js(&indicators)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostCapabilities {
    npc_decision_diagnostics: bool,
}

#[wasm_bindgen]
pub fn host_capabilities() -> Result<JsValue, JsValue> {
    to_js(&HostCapabilities {
        npc_decision_diagnostics: cfg!(all(feature = "simulation-diagnostics", debug_assertions)),
    })
}

#[cfg(all(feature = "simulation-diagnostics", debug_assertions))]
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
            .map_err(session_error_to_js)
    })
}

/// 销毁会话（释放内存）。
#[wasm_bindgen]
pub fn drop_session(handle: u32) {
    REGISTRY.with(|r| {
        r.borrow_mut().remove(handle);
    });
}

/// 返回最近完成自然日的存档候选；首个日终完成前返回错误。
#[wasm_bindgen]
pub fn save(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |sess| {
        let slot = sess.save().map_err(session_error_to_js)?;
        save_to_js(&slot)
    })
}

/// 仅捕获指定 CivilUpdate seq 与 settledDate 对应的已完成日终候选。
#[wasm_bindgen]
pub fn save_candidate(handle: u32, key: JsValue) -> Result<JsValue, JsValue> {
    let key: SaveCandidateKey = serde_wasm_bindgen::from_value(key)?;
    with_session(handle, |sess| {
        let slot = sess.save_candidate(&key).map_err(session_error_to_js)?;
        save_to_js(&slot)
    })
}

/// 从存档恢复（精确到天）。返回新句柄。
#[wasm_bindgen]
pub fn restore(save_slot: JsValue) -> Result<u32, JsValue> {
    let slot: SaveSlot = serde_wasm_bindgen::from_value(save_slot)?;
    REGISTRY
        .with(|registry| registry.borrow_mut().restore(&slot))
        .map_err(session_error_to_js)
}

#[wasm_bindgen]
pub fn restore_json(save_json: String) -> Result<u32, JsValue> {
    let slot: SaveSlot =
        serde_json::from_str(&save_json).map_err(|error| JsValue::from_str(&error.to_string()))?;
    REGISTRY
        .with(|registry| registry.borrow_mut().restore(&slot))
        .map_err(session_error_to_js)
}

/// 句柄内执行闭包；句柄无效 → 抛 JsValue。
fn with_session<T>(
    handle: u32,
    f: impl FnOnce(&mut ProtocolSession) -> Result<T, JsValue>,
) -> Result<T, JsValue> {
    REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .with_session(handle, f)
            .map_err(|message| JsValue::from_str(&message))?
    })
}

#[cfg(test)]
mod error_layout_tests {
    use super::*;

    #[test]
    fn step_update_error_keeps_fatal_payload_indirect() {
        assert!(std::mem::size_of::<StepUpdateError>() <= 2 * std::mem::size_of::<String>());
    }
}
