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

use engine::company::{PublicReportAvailability, PublicReportAvailabilityQuery, PublicReportPage, PublicReportQuery, PublicReportSummary};
use engine::session::protocol::{EngineUpdate, ProtocolSession, SaveCandidateKey};
use engine::{AccountId, Intent, SaveSlot, SessionError, SessionSetup};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{hash_map::Entry, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use wasm_bindgen::prelude::*;

// Export the Rayon bootstrap itself; the Web Worker awaits its Promise before
// constructing a session, so initialization errors cannot be silently lost.
#[cfg(target_arch = "wasm32")]
pub use wasm_bindgen_rayon::init_thread_pool;

/// 序列化为 JsValue。map 默认序列化为 JS Map（AccountId 是数字键，无法作 Object 键）；
/// 前端 host 适配器负责把 Map 规整为普通对象（Object.fromEntries）供 React/RTK 消费。
/// `?Sized` 允许直接序列化切片引用（如偏好台账 `&[SimplePreferenceRejection]`）。
fn to_js<T: Serialize + ?Sized>(v: &T) -> Result<JsValue, JsValue> {
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
static INGRESS: Mutex<Option<HashMap<u32, engine::SharedSessionIngress>>> = Mutex::new(None);

fn ingress_operation<T>(
    operation: impl FnOnce(&mut HashMap<u32, engine::SharedSessionIngress>) -> Result<T, SessionError>,
) -> Result<T, SessionError> {
    let mut registry = INGRESS.lock().map_err(|_| {
        SessionError::ResourceLimit("WASM ingress registry 锁已损坏，请重新启动会话".into())
    })?;
    operation(registry.get_or_insert_with(HashMap::new))
}

fn reserve_session_handle(next: &AtomicU32) -> Result<u32, SessionError> {
    next.fetch_update(
        Ordering::SeqCst,
        Ordering::SeqCst,
        |candidate| match candidate {
            0 => None,
            u32::MAX => Some(0),
            _ => Some(candidate + 1),
        },
    )
    .map_err(|_| {
        SessionError::ResourceLimit(
            "WASM 会话句柄已耗尽；已有会话仍有效，新建会话须重新启动 Worker".into(),
        )
    })
}

/// WASM Worker 局部的句柄 owner；ProtocolSession 仍拥有引擎状态。
#[derive(Default)]
struct SessionRegistry {
    sessions: HashMap<u32, ProtocolSession>,
}

impl SessionRegistry {
    fn register(
        &mut self,
        session: ProtocolSession,
        next: &AtomicU32,
    ) -> Result<u32, SessionError> {
        let id = reserve_session_handle(next)?;
        match self.sessions.entry(id) {
            Entry::Vacant(entry) => {
                ingress_operation(|registry| {
                    if registry.contains_key(&id) {
                        return Err(SessionError::ResourceLimit(format!("WASM ingress token {id} 已登记")));
                    }
                    registry.insert(id, session.shared_ingress());
                    Ok(())
                })?;
                entry.insert(session);
                Ok(id)
            }
            Entry::Occupied(_) => Err(SessionError::Step(
                engine::session::StepFatal::InvariantViolation {
                    location: "SessionRegistry::register".into(),
                    description: format!("WASM 会话句柄 {id} 已登记，禁止覆盖现有会话"),
                },
            )),
        }
    }

    fn create(&mut self, setup: SessionSetup, seed: u64) -> Result<u32, SessionError> {
        let session = ProtocolSession::new(setup, seed)?;
        self.register(session, &NEXT)
    }

    fn restore(&mut self, slot: &SaveSlot) -> Result<u32, SessionError> {
        let session = ProtocolSession::restore(slot)?;
        self.register(session, &NEXT)
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

    fn remove(&mut self, handle: u32) -> Result<(), SessionError> {
        if !self.sessions.contains_key(&handle) {
            return Ok(());
        }
        ingress_operation(|registry| {
            if let Some(service) = registry.remove(&handle) {
                service.close()?;
            }
            Ok(())
        })?;
        self.sessions.remove(&handle);
        Ok(())
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
            StepUpdateError::Fatal(failure) => StepUpdateError::Fatal(Box::new((*failure).at_session(session))),
            StepUpdateError::Recoverable(failure) => StepUpdateError::Recoverable(Box::new((*failure).at_session(session))),
            other => other,
        })
    }

    fn submit_report_correction(
        &mut self,
        handle: u32,
        request: engine::CompanyReportCorrection,
    ) -> Result<(), StepUpdateError> {
        self.with_session(handle, |session| {
            let epoch = session.report_correction_epoch();
            session.enqueue_report_correction(&epoch, request)
        })
        .map_err(StepUpdateError::Operation)?
        .map_err(session_error_to_step_update_error)
    }

    fn cancel_report_correction(
        &mut self,
        handle: u32,
        operation_id: &str,
    ) -> Result<(), StepUpdateError> {
        self.with_session(handle, |session| {
            let epoch = session.report_correction_epoch();
            session.cancel_report_correction(&epoch, operation_id)
        })
        .map_err(StepUpdateError::Operation)?
        .map_err(session_error_to_step_update_error)
    }

    fn query_report_corrections(
        &mut self,
        handle: u32,
    ) -> Result<engine::ReportCorrectionStatus, StepUpdateError> {
        self.with_session(handle, |session| session.report_corrections())
            .map_err(StepUpdateError::Operation)?
            .map_err(session_error_to_step_update_error)
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
            SessionError::ReportCorrection(_) => Self {
                code: "REPORT_CORRECTION_REJECTED",
                message: error.to_string(),
                r#where: "web-wasm.end_civil_day_update".into(),
                cause: failure_cause(&error),
                context: FailureContext::new("endCivilDay"),
                recoverable: true,
                recovery_actions: vec![
                    "查询并取消错误的待处理更正后重试日结",
                    "复制更正操作身份与错误详情反馈",
                ],
            },
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
    Recoverable(Box<HostFailure>),
}

impl std::fmt::Display for StepUpdateError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Operation(message) => formatter.write_str(message),
            Self::Fatal(failure) | Self::Recoverable(failure) => formatter.write_str(&failure.message),
        }
    }
}

fn step_update_error_to_js(error: StepUpdateError) -> JsValue {
    match error {
        StepUpdateError::Operation(message) => JsValue::from_str(&message),
        StepUpdateError::Fatal(failure) | StepUpdateError::Recoverable(failure) => to_js(&*failure).unwrap_or_else(|serialize_error| {
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
    if matches!(&error, SessionError::ReportCorrection(_)) {
        StepUpdateError::Recoverable(Box::new(HostFailure::civil(error)))
    } else {
        StepUpdateError::Fatal(Box::new(HostFailure::civil(error)))
    }
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
            let failure = HostFailure::civil(error).at_session(sess);
            let error = if failure.recoverable {
                StepUpdateError::Recoverable(Box::new(failure))
            } else {
                StepUpdateError::Fatal(Box::new(failure))
            };
            step_update_error_to_js(error)
        })?;
        to_js(&EngineUpdate::CivilUpdate(Box::new(report)))
    })
}

#[wasm_bindgen]
pub fn submit_report_correction(handle: u32, request: JsValue) -> Result<(), JsValue> {
    let request: engine::CompanyReportCorrection = serde_wasm_bindgen::from_value(request)?;
    REGISTRY
        .with(|registry| {
            registry
                .borrow_mut()
                .submit_report_correction(handle, request)
        })
        .map_err(step_update_error_to_js)
}

#[wasm_bindgen]
pub fn cancel_report_correction(handle: u32, operation_id: String) -> Result<(), JsValue> {
    REGISTRY
        .with(|registry| {
            registry
                .borrow_mut()
                .cancel_report_correction(handle, &operation_id)
        })
        .map_err(step_update_error_to_js)
}

#[wasm_bindgen]
pub fn query_report_corrections(handle: u32) -> Result<JsValue, JsValue> {
    let status = REGISTRY
        .with(|registry| registry.borrow_mut().query_report_corrections(handle))
        .map_err(step_update_error_to_js)?;
    status
        .serialize(&serde_wasm_bindgen::Serializer::new().serialize_maps_as_objects(true))
        .map_err(|error| JsValue::from_str(&error.to_string()))
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

/// 查询指定财报期间、类型与 scope 的公开可用性。
#[wasm_bindgen]
pub fn public_report_availability(handle: u32, query: JsValue) -> Result<JsValue, JsValue> {
    let query: PublicReportAvailabilityQuery = serde_wasm_bindgen::from_value(query)?;
    with_session(handle, |sess| {
        let availability: PublicReportAvailability = sess
            .query_public_report_availability(&query)
            .map_err(session_error_to_js)?;
        public_dto_to_js(&availability)
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
pub fn query_stock_history(handle: u32, code: String) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let history = session
            .query_stock_history(engine::AccountId(0), &engine::StockCode(code))
            .map_err(session_error_to_js)?;
        public_dto_to_js(&history)
    })
}

#[wasm_bindgen]
pub fn initial_allocation(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let allocation = session.initial_allocation().map_err(session_error_to_js)?;
        public_dto_to_js(&allocation)
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
    indicator_capabilities: IndicatorCapabilities,
    personal_trade_history: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct IndicatorCapabilities {
    intraday_average: bool,
    macd: bool,
    price_kdj: bool,
    candle_kdj: bool,
}

#[wasm_bindgen]
pub fn host_capabilities() -> Result<JsValue, JsValue> {
    to_js(&HostCapabilities {
        npc_decision_diagnostics: cfg!(all(feature = "simulation-diagnostics", debug_assertions)),
        indicator_capabilities: IndicatorCapabilities {
            intraday_average: true,
            macd: true,
            price_kdj: true,
            candle_kdj: true,
        },
        personal_trade_history: true,
    })
}

#[wasm_bindgen]
pub fn calculate_intraday_average(
    turnover_cents: String,
    trade_count: u64,
    volume_shares: u64,
) -> Result<JsValue, JsValue> {
    let turnover_cents = engine::parse_turnover_cents(&turnover_cents)
        .map_err(|error| JsValue::from_str(&format!("turnover_cents 无效：{error}")))?;
    let stats = engine::DailyTradeStats { turnover_cents, trade_count };
    let average = engine::calculate_intraday_average(Some(&stats), volume_shares)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    to_js(&average)
}

#[wasm_bindgen]
pub fn calculate_intraday_average_curve(samples: JsValue) -> Result<JsValue, JsValue> {
    let samples: Vec<engine::IntradayAverageInput> = serde_wasm_bindgen::from_value(samples)?;
    if samples.len() > 600 {
        return Err(JsValue::from_str("VWAP curve 样本数不能超过 600"));
    }
    let curve = engine::calculate_intraday_average_curve(&samples)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    to_js(&curve)
}

#[wasm_bindgen]
pub fn personal_trade_confirmations(handle: u32, before_receipt: Option<String>) -> Result<JsValue, JsValue> {
    let before_receipt = before_receipt.map(|value| {
        if value.is_empty() || value.len() > 20 || !value.bytes().all(|byte| byte.is_ascii_digit()) || (value.len() > 1 && value.starts_with('0')) {
            return Err(JsValue::from_str("交割单 before_receipt 必须是规范u64字符串"));
        }
        value.parse::<u64>().map_err(|error| JsValue::from_str(&error.to_string()))
    }).transpose()?;
    with_session(handle, |session| {
        to_js(&session.personal_trade_confirmations_page(AccountId(0), before_receipt))
    })
}

/// 查询本机玩家（固定 AccountId(0)）的股息税状态：会话税务模式、纳税人身份分类
/// 与每个已配置完整名册证券上的税账状态。只读汇总，不产生新事实。
#[wasm_bindgen]
pub fn owner_dividend_tax_status(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let view = session
            .account_dividend_tax_status(AccountId(0))
            .map_err(session_error_to_js)?;
        to_js(&view)
    })
}

/// 查询本机玩家（固定 AccountId(0)）各证券的个人现金分红税未划收税额与资金不足原因；
/// 消费 `GameSession::dividend_tax_outstanding_views` 并按 owner 过滤，只读不落新事实。
#[wasm_bindgen]
pub fn owner_dividend_tax_outstanding_views(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let views = session
            .dividend_tax_outstanding_views()
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        let owner_views: Vec<_> = views
            .into_iter()
            .filter(|view| view.account == AccountId(0))
            .collect();
        to_js(&owner_views)
    })
}

/// 显式配置账户在指定证券下的现金分红税务身份（宿主显式入口）。
/// 仅限装配期语义：开局后（名册已有历史日结回执或已登记分红）配置会被 engine
/// 显式拒绝，完整错误信息直接抛给宿主展示，不静默降级。
#[wasm_bindgen]
pub fn configure_dividend_tax_book(
    handle: u32,
    account: String,
    stock: String,
    profile: JsValue,
) -> Result<(), JsValue> {
    if account.is_empty()
        || account.len() > 20
        || !account.bytes().all(|byte| byte.is_ascii_digit())
        || (account.len() > 1 && account.starts_with('0'))
    {
        return Err(JsValue::from_str("股息税账户必须是规范u64非负十进制字符串"));
    }
    let account = account
        .parse::<u64>()
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    if stock.trim().is_empty() {
        return Err(JsValue::from_str("股息税证券代码不能为空"));
    }
    let profile: engine::company::cash_dividend_tax::DividendTaxProfile =
        serde_wasm_bindgen::from_value(profile)
            .map_err(|error| JsValue::from_str(&format!("股息税身份无效：{error}")))?;
    with_session(handle, |session| {
        session
            .configure_cash_dividend_tax_book(
                AccountId(account),
                engine::StockCode(stock),
                profile,
            )
            .map_err(|error| JsValue::from_str(&error.to_string()))
    })
}

/// 查询本机玩家（固定 AccountId(0)）的配股认购拒绝回执（M 批公开配售超额认购
/// 极端竞态兜底的持久化留痕）；只读过滤既有事实，不产生新事实。
#[wasm_bindgen]
pub fn owner_rejected_rights_subscriptions(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let owner = AccountId(0);
        let rejected: Vec<_> = session
            .game()
            .corporate_actions()
            .rejected_rights_subscriptions
            .iter()
            .filter(|receipt| receipt.account == owner)
            .cloned()
            .collect();
        to_js(&rejected)
    })
}

/// 玩家（固定 AccountId(0)，owner 隔离）显式提交配股认购（参数 = 配股事件 +
/// 认购股数）。受理成功返回排队认购回执（当日日终划扣，恰好一条——engine
/// 拒绝同一持有人重复提交）；现金不足、缴款窗口外、额度不足、重复提交等由
/// engine 显式拒绝，完整错误信息直接抛给宿主展示，不静默降级。
/// `shares` 必须是规范 u64 非负十进制字符串（与股数 wire 口径一致）。
#[wasm_bindgen]
pub fn subscribe_rights_offering(
    handle: u32,
    event_id: String,
    shares: String,
) -> Result<JsValue, JsValue> {
    if event_id.trim().is_empty() || event_id.encode_utf16().count() > 128 {
        return Err(JsValue::from_str(
            "认购配股事件身份必须是非空且不超过 128 字符的字符串",
        ));
    }
    if shares.is_empty()
        || shares.len() > 20
        || !shares.bytes().all(|byte| byte.is_ascii_digit())
        || (shares.len() > 1 && shares.starts_with('0'))
    {
        return Err(JsValue::from_str(
            "认购股数必须是规范 u64 非负十进制字符串",
        ));
    }
    let shares = shares
        .parse::<u64>()
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    with_session(handle, |session| {
        let owner = AccountId(0);
        session
            .subscribe_rights_offering(&event_id, owner, shares)
            .map_err(session_error_to_js)?;
        // 受理成功后 (event_id, owner) 在排队队列中恰好一条（重复提交被
        // engine 拒绝），据此取回受理回执事实。
        let receipt = session
            .game()
            .corporate_actions()
            .rights_subscription_queue
            .iter()
            .find(|queued| queued.event_id == event_id && queued.account == owner)
            .cloned()
            .ok_or_else(|| {
                JsValue::from_str("配股认购受理成功但排队回执缺失（引擎状态不一致）")
            })?;
        to_js(&receipt)
    })
}

/// 查询某公司行为偏好自动提案的拒绝台账（ADR-0037）。只读读取最近一次已完成
/// 自然日日终存档中的公司系统状态（偏好台账只在日结候选事务内变更）；首个日终
/// 完成前显式报错，不静默返回空台账冒充「无拒绝」。
#[wasm_bindgen]
pub fn company_preference_rejections(handle: u32, company: String) -> Result<JsValue, JsValue> {
    // 与 TS 侧（wasm-worker/worker-host）同口径：按 UTF-16 码元计数上限 64，
    // 避免多字节身份在三层的长度单位不一致。
    if company.trim().is_empty() || company.encode_utf16().count() > 64 {
        return Err(JsValue::from_str("偏好台账公司身份必须是非空且不超过 64 字符的字符串"));
    }
    with_session(handle, |session| {
        let slot: SaveSlot = session.save().map_err(session_error_to_js)?;
        let company_id = engine::company::CompanyId(company);
        let rejections = slot
            .company_system
            .preference_rejections(&company_id)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        to_js(rejections)
    })
}

/// 查询某公司的完整共同契约能力面（F 批收口）：公司当前事实 + 未完成方案 +
/// 各行为业务条件 + 本人（固定 AccountId(0)）权利摘要。owner 隔离，只读投影。
#[wasm_bindgen]
pub fn company_capabilities(handle: u32, company: String) -> Result<JsValue, JsValue> {
    if company.trim().is_empty() || company.encode_utf16().count() > 64 {
        return Err(JsValue::from_str("能力面公司身份必须是非空且不超过 64 字符的字符串"));
    }
    with_session(handle, |session| {
        let view = session
            .company_capabilities(&engine::company::CompanyId(company), AccountId(0))
            .map_err(session_error_to_js)?;
        to_js(&view)
    })
}

/// 按公司+期间读取期间变化解释（复用既有 history；period_end 为 ISO YYYY-MM-DD
/// 且必须是结算周期末日，否则显式拒绝）。
#[wasm_bindgen]
pub fn company_period_explanation(
    handle: u32,
    company: String,
    period_end: String,
) -> Result<JsValue, JsValue> {
    if company.trim().is_empty() || company.encode_utf16().count() > 64 {
        return Err(JsValue::from_str("解释查询公司身份必须是非空且不超过 64 字符的字符串"));
    }
    let period_end = engine::calendar::CivilDate::from_iso(&period_end)
        .map_err(|error| JsValue::from_str(&format!("解释查询期间末日无效：{error}")))?;
    with_session(handle, |session| {
        let explanation = session
            .company_period_explanation(&engine::company::CompanyId(company), period_end)
            .map_err(session_error_to_js)?;
        to_js(&explanation)
    })
}

/// 查询本机玩家（固定 AccountId(0)）的未完成配股权证/额度/缴款窗口视图；
/// 只读投影既有 books/queue 事实，Settled 终态不出现。
#[wasm_bindgen]
pub fn owner_rights_offerings(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let views = session
            .owner_rights_offerings(AccountId(0))
            .map_err(session_error_to_js)?;
        to_js(&views)
    })
}

/// 查询本机玩家（固定 AccountId(0)）的简税（FlatWithholding）付款日代扣回执；
/// 非 Flat 模式由 engine 显式拒绝，不冒充空台账。
#[wasm_bindgen]
pub fn owner_flat_withholding_receipts(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let receipts = session
            .owner_flat_withholding_receipts(AccountId(0))
            .map_err(session_error_to_js)?;
        to_js(&receipts)
    })
}

#[wasm_bindgen]
pub fn personal_trade_history(handle: u32, query: JsValue) -> Result<JsValue, JsValue> {
    let request: engine::session::PersonalTradeHistoryRequest = serde_wasm_bindgen::from_value(query)?;
    with_session(handle, |session| {
        let page = session.query_personal_trade_history(AccountId(0), request)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        to_js(&page)
    })
}

#[wasm_bindgen]
pub fn market_history(handle: u32, query: JsValue) -> Result<JsValue, JsValue> {
    let request: engine::session::MarketHistoryRequest = serde_wasm_bindgen::from_value(query)?;
    with_session(handle, |session| {
        let page = session.query_market_history_for(AccountId(0), &request)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        to_js(&page)
    })
}

#[wasm_bindgen]
pub fn current_minute_history(handle: u32, query: JsValue) -> Result<JsValue, JsValue> {
    let request: engine::session::CurrentMinuteHistoryRequest = serde_wasm_bindgen::from_value(query)?;
    with_session(handle, |session| {
        let response = session.query_current_minute_history_for(AccountId(0), &request)
            .map_err(|error| JsValue::from_str(&error.to_string()))?;
        to_js(&response)
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

#[wasm_bindgen]
pub fn ingress_token(handle: u32) -> Result<u32, JsValue> {
    with_session(handle, |_| Ok(handle))
}

#[wasm_bindgen]
pub fn ingress_enqueue(token: u32, intent: JsValue) -> Result<(), JsValue> {
    let intent: Intent = serde_wasm_bindgen::from_value(intent)?;
    let service = ingress_operation(|registry| {
        registry.get(&token).cloned().ok_or_else(|| {
            SessionError::ResourceLimit(format!("WASM ingress token {token} 已关闭或不存在"))
        })
    }).map_err(session_error_to_js)?;
    service.enqueue_player_intent(AccountId(0), intent).map_err(session_error_to_js)
}

#[cfg(feature = "verification-harness")]
fn verification_ingress_source(token: u32) -> Result<engine::SharedSessionIngress, JsValue> {
    ingress_operation(|registry| {
        registry.get(&token).cloned().ok_or_else(|| {
            SessionError::ResourceLimit(format!("WASM verification ingress token {token} 已关闭或不存在"))
        })
    })
    .map_err(session_error_to_js)
}

#[cfg(feature = "verification-harness")]
#[wasm_bindgen]
pub fn verification_ingress_arm(
    token: u32,
    phase: String,
    tick: u64,
    account: Option<u64>,
) -> Result<(), JsValue> {
    verification_ingress_source(token)?
        .verification_arm(&phase, tick, account.map(AccountId))
        .map_err(session_error_to_js)
}

#[cfg(feature = "verification-harness")]
#[wasm_bindgen]
pub fn verification_ingress_snapshot(token: u32) -> Result<JsValue, JsValue> {
    let snapshot = verification_ingress_source(token)?
        .verification_snapshot()
        .map_err(session_error_to_js)?;
    to_js(&snapshot)
}

#[cfg(feature = "verification-harness")]
#[wasm_bindgen]
pub fn verification_ingress_release(token: u32) -> Result<(), JsValue> {
    verification_ingress_source(token)?
        .verification_release()
        .map_err(session_error_to_js)
}

#[cfg(feature = "verification-harness")]
#[wasm_bindgen]
pub fn verification_resting_orders(
    handle: u32,
    account: u64,
    code: String,
) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let save = session
            .game()
            .save()
            .map_err(|fatal| session_error_to_js(SessionError::from(fatal)))?;
        let owner = AccountId(account);
        let stock = engine::StockCode(code);
        if !save.snapshot.accounts.contains_key(&owner) {
            return Err(JsValue::from_str(&format!("verification account {account} 不存在")));
        }
        if !save.snapshot.markets.contains_key(&stock) {
            return Err(JsValue::from_str(&format!("verification stock {} 不存在", stock.0)));
        }
        let orders = save.resting_orders.get(&stock).ok_or_else(|| {
            JsValue::from_str(&format!("verification stock {} 缺少真实订单簿投影", stock.0))
        })?;
        let owned = orders.iter().filter(|order| order.owner == owner).cloned().collect::<Vec<_>>();
        to_js(&owned)
    })
}

/// 销毁会话（释放内存）。
#[wasm_bindgen]
pub fn drop_session(handle: u32) -> Result<(), JsValue> {
    REGISTRY.with(|r| {
        r.borrow_mut().remove(handle)
    }).map_err(session_error_to_js)
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
