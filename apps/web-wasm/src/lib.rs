//! engine → WASM 绑定（ADR-0007 §4）。
//!
//! native 直接使用 std::thread 运行时（ADR-0043）；WASM 的 `World` 也保留，
//! worker 创建经 `thread_facade` 接入；宿主可在首次读取前显式设置一次
//! `step runtime override` 进入该路径。缺省仍走串行管线，尚未翻转为 runtime；
//! WASM 多 worker 所需的 `atomics`、`build-std` 与 COOP/COEP 隔离链仍是相关条件，
//! 不代表已删除或本片已验证。WASM 默认串行属于简化（见 docs/trading-rules 与
//! memory/project_std_thread_migration）；本片未实跑 WASM cfg、ABI、`World` 或 Browser。
//!
//! GameSession 不可序列化（含 `Box<dyn Strategy + Send + Sync>`），故存于 thread_local
//! 句柄注册表；仅 `Snapshot`/`Event`/`Intent`/`SessionSetup` 经 serde-wasm-bindgen 跨界。
//! JS 持 u32 句柄调 create/step/snapshot/enqueue/drop。
//!
//! 纯前端单机：player 固定 AccountId(0)（enqueue 不带 player_id）。

use engine::company::{PublicReportPage, PublicReportQuery, PublicReportSummary};
use engine::session::protocol::{EngineUpdate, ProtocolSession, SaveCandidateKey};
use engine::session::{
    CurrentMinuteHistoryRequest, MarketHistoryRequest, PersonalTradeHistoryRequest,
};
use engine::{AccountId, Intent, SaveSlot, SessionError, SessionSetup};
use serde::Serialize;
use std::cell::RefCell;
use std::collections::{hash_map::Entry, HashMap};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;
use wasm_bindgen::prelude::*;

/// 在 WASM 首次读取运行时开关前设置一次显式 override。
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn set_step_runtime_override(enabled: bool) -> Result<(), JsValue> {
    engine::runtime::set_step_runtime_override(enabled).map_err(JsValue::from_str)
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

#[derive(Debug, Serialize)]
struct CompanyReadResponse {
    owner: AccountId,
    value: engine::session::protocol::CompanyReadResult,
}

fn query_company_reads(
    session: &ProtocolSession,
    query: engine::session::protocol::CompanyReadQuery,
) -> Result<CompanyReadResponse, engine::session::protocol::CompanyReadError> {
    let owner = AccountId(0);
    session
        .company_read_query(owner, query)
        .map(|value| CompanyReadResponse { owner, value })
}

/// 只读公司事实；句柄来自实际 registry，账户不由 JS 请求指定。
#[wasm_bindgen]
pub fn company_read_query(handle: u32, query: JsValue) -> Result<JsValue, JsValue> {
    let query = serde_wasm_bindgen::from_value(query)?;
    with_session(handle, |session| {
        match query_company_reads(session, query) {
            Ok(response) => public_dto_to_js(&response),
            Err(error) => Err(public_dto_to_js(&error.public_failure())?),
        }
    })
}

fn propose_company_action(
    session: &mut ProtocolSession,
    proposal: engine::session::PlayerCompanyProposal,
) -> Result<
    engine::session::PlayerCompanyCommandReceipt,
    engine::session::protocol::CompanyWriteError,
> {
    session
        .propose_player_company_action(AccountId(0), proposal)
        .map_err(Into::into)
}

fn enqueue_correction(
    session: &mut ProtocolSession,
    request: engine::session::CompanyReportCorrection,
) -> Result<
    engine::session::protocol::ReportCorrectionAcknowledgement,
    engine::session::protocol::CompanyWriteError,
> {
    let epoch = session.game().report_correction_epoch();
    let operation_id = request.operation_id.clone();
    session.enqueue_report_correction(&epoch, request)?;
    Ok(engine::session::protocol::ReportCorrectionAcknowledgement { operation_id })
}

fn cancel_correction(
    session: &mut ProtocolSession,
    operation_id: &str,
) -> Result<
    engine::session::protocol::ReportCorrectionAcknowledgement,
    engine::session::protocol::CompanyWriteError,
> {
    let epoch = session.game().report_correction_epoch();
    session.cancel_report_correction(&epoch, operation_id)?;
    Ok(engine::session::protocol::ReportCorrectionAcknowledgement {
        operation_id: operation_id.to_owned(),
    })
}

fn company_write_error_to_js(
    error: engine::session::protocol::CompanyWriteError,
    session: &ProtocolSession,
) -> Result<JsValue, JsValue> {
    // JavaScript 只接收公开投影；可信 console 保留本地 Session 的原 typed cause。
    log_company_write_error(&error);
    if let Some(fatal) = session
        .require_healthy()
        .err()
        .or_else(|| error.step_fatal().cloned())
    {
        return Err(step_update_error_to_js(StepUpdateError::Fatal(Box::new(
            HostFailure::step(fatal).at_session(session),
        ))));
    }
    Err(public_dto_to_js(&error.public_failure())?)
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = error)]
    fn log_company_write_error_message(message: &str);
}

fn log_company_write_error(error: &engine::session::protocol::CompanyWriteError) {
    #[cfg(target_arch = "wasm32")]
    log_company_write_error_message(&format!("web-wasm 公司写命令拒绝：{error:?}"));
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("web-wasm 公司写命令拒绝：{error:?}");
}

#[wasm_bindgen]
pub fn propose_player_company_action(handle: u32, proposal: JsValue) -> Result<JsValue, JsValue> {
    let proposal = serde_wasm_bindgen::from_value(proposal)?;
    with_session(handle, |session| {
        match propose_company_action(session, proposal) {
            Ok(receipt) => public_dto_to_js(&serde_json::json!({ "receipt": receipt })),
            Err(error) => company_write_error_to_js(error, session),
        }
    })
}

#[wasm_bindgen]
pub fn enqueue_report_correction(handle: u32, request: JsValue) -> Result<JsValue, JsValue> {
    let request = serde_wasm_bindgen::from_value(request)?;
    with_session(handle, |session| {
        match enqueue_correction(session, request) {
            Ok(acknowledgement) => public_dto_to_js(&acknowledgement),
            Err(error) => company_write_error_to_js(error, session),
        }
    })
}

#[wasm_bindgen]
pub fn cancel_report_correction(handle: u32, operation_id: String) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        match cancel_correction(session, &operation_id) {
            Ok(acknowledgement) => public_dto_to_js(&acknowledgement),
            Err(error) => company_write_error_to_js(error, session),
        }
    })
}

#[cfg(test)]
mod company_write_tests;

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

#[derive(Debug)]
struct UnsupportedRuntimeSyncPolicy {
    code: &'static str,
    policy: engine::RuntimeSyncPolicy,
}

impl std::fmt::Display for UnsupportedRuntimeSyncPolicy {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "{}：WASM 仅支持 every_tick，当前策略 {:?} 不受支持",
            self.code, self.policy
        )
    }
}

fn require_wasm_runtime_sync_policy(
    policy: engine::RuntimeSyncPolicy,
) -> Result<(), UnsupportedRuntimeSyncPolicy> {
    match policy {
        engine::RuntimeSyncPolicy::EveryTick => Ok(()),
        unsupported => Err(UnsupportedRuntimeSyncPolicy {
            code: "WASM_RUNTIME_SYNC_POLICY_UNSUPPORTED",
            policy: unsupported,
        }),
    }
}

#[derive(Debug)]
enum HostSessionInitError {
    UnsupportedRuntimeSyncPolicy(UnsupportedRuntimeSyncPolicy),
    Session(SessionError),
}

impl std::fmt::Display for HostSessionInitError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnsupportedRuntimeSyncPolicy(error) => std::fmt::Display::fmt(error, formatter),
            Self::Session(error) => std::fmt::Display::fmt(error, formatter),
        }
    }
}

impl From<SessionError> for HostSessionInitError {
    fn from(error: SessionError) -> Self {
        Self::Session(error)
    }
}

fn host_session_init_error_to_js(error: HostSessionInitError) -> JsValue {
    match error {
        HostSessionInitError::UnsupportedRuntimeSyncPolicy(error) => {
            JsValue::from_str(&error.to_string())
        }
        HostSessionInitError::Session(error) => session_error_to_js(error),
    }
}

fn ingress_operation<T>(
    operation: impl FnOnce(&mut HashMap<u32, engine::SharedSessionIngress>) -> Result<T, SessionError>,
) -> Result<T, SessionError> {
    let mut registry = INGRESS.lock().map_err(|_| {
        // 1300：WASM ingress registry 锁中毒。
        SessionError::ResourceLimit(engine::error::ValidationFault::bare(1300))
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
        // 1301：WASM 会话句柄耗尽。
        SessionError::ResourceLimit(engine::error::ValidationFault::bare(1301))
    })
}

/// WASM Worker 局部的句柄 owner；ProtocolSession 仍拥有引擎状态。
#[derive(Default)]
struct SessionRegistry {
    sessions: HashMap<u32, ProtocolSession>,
}

mod shutdown;

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
                        // 1302：ingress token 重复登记；ctx[0]=token。
                        return Err(SessionError::ResourceLimit(
                            engine::error::ValidationFault::new(1302, vec![u64::from(id)]),
                        ));
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

    fn create(&mut self, setup: SessionSetup, seed: u64) -> Result<u32, HostSessionInitError> {
        require_wasm_runtime_sync_policy(setup.runtime_sync_policy)
            .map_err(HostSessionInitError::UnsupportedRuntimeSyncPolicy)?;
        let session = ProtocolSession::new(setup, seed)?;
        self.register(session, &NEXT).map_err(Into::into)
    }

    fn restore(&mut self, slot: &SaveSlot) -> Result<u32, HostSessionInitError> {
        require_wasm_runtime_sync_policy(slot.setup.runtime_sync_policy)
            .map_err(HostSessionInitError::UnsupportedRuntimeSyncPolicy)?;
        let session = ProtocolSession::restore(slot)?;
        self.register(session, &NEXT).map_err(Into::into)
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
    pub fault: Option<engine::ValidationFault>,
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
            SessionError::ReportCorrectionDayEnd(source) => failure_description(source),
            SessionError::CivilInformation(_) => (
                "CIVIL_INFORMATION_FAILED",
                "自然日信息事务失败（内部详情已脱敏）",
            ),
            SessionError::Closing(_) => (
                "SESSION_CLOSING_FAILED",
                "自然日日终封账失败（私有详情已脱敏）",
            ),
            SessionError::ReportCorrectionRestore(_) => (
                "SESSION_STATE_INVALID",
                "更正完成事实与真实会计和公开版本不一致（原始详情已脱敏）",
            ),
            SessionError::InvalidSave(fault)
                if fault.code == engine::error::SAVE_TARGET_DAY_NOT_READY =>
            {
                (
                    "SAVE_TARGET_DAY_NOT_READY",
                    "目标自然日尚未完成日终，当前日存档不可用（原始详情已脱敏）",
                )
            }
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
    if let Some(error) = error.downcast_ref::<engine::session::ReportCorrectionDayEndError>() {
        return if error.classification() == engine::company::CompanyErrorClass::SystemState {
            (
                "REPORT_CORRECTION_FAILED",
                "报表更正日终准备失败（内部详情已脱敏）",
            )
        } else {
            (
                "REPORT_CORRECTION_REJECTED",
                "报表更正未满足提交条件（内部详情已脱敏）",
            )
        };
    }
    if error.is::<engine::session::CivilInformationError>() {
        return (
            "CIVIL_INFORMATION_FAILED",
            "自然日信息事务失败（内部详情已脱敏）",
        );
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
    if error.is::<engine::session::ReportCorrectionRestoreError>() {
        return (
            "REPORT_CORRECTION_RESTORE_FAILED",
            "报表更正存档关联校验失败（原始详情已脱敏）",
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
            fault: None,
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
    pub(crate) fn catalog(
        error: &engine::session::CompanyCatalogViewError,
        operation: &'static str,
        location: &'static str,
    ) -> Self {
        let mut context = FailureContext::new(operation);
        context.fault = Some(engine::session::company_catalog_fault(error));
        Self {
            code: "CATALOG_BASELINE_FAILED",
            message: "公开公司身份目录构造失败".to_owned(),
            r#where: location.to_owned(),
            cause: None,
            context,
            recoverable: false,
            recovery_actions: vec![
                "停止当前提交并重新同步权威会话；仍失败则重开有效日终档或新局",
                "复制错误详情反馈",
            ],
        }
    }

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
        SessionError::InvalidSave(ref fault)
            if fault.code == engine::error::SAVE_TARGET_DAY_NOT_READY =>
        {
            let (code, message) = failure_description(&error);
            StepUpdateError::Operation(format!("{code}: {message}"))
        }
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
        .map_err(host_session_init_error_to_js)
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
pub fn stock_catalog_view(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |session| to_js(&session.stock_catalog_view()))
}

#[wasm_bindgen]
pub fn company_catalog_view(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let catalog = session.company_catalog_view();
        match finish_company_catalog(session, catalog) {
            Ok(catalog) => to_js(&catalog),
            Err(failure) => Err(to_js(&failure)?),
        }
    })
}

fn finish_company_catalog(
    session: &ProtocolSession,
    catalog: Result<
        Vec<engine::session::CompanyCatalogEntry>,
        engine::session::CompanyCatalogViewError,
    >,
) -> Result<Vec<engine::session::CompanyCatalogEntry>, Box<HostFailure>> {
    catalog.map_err(|error| {
        let mut failure = HostFailure::catalog(
            &error,
            "company_catalog_view",
            "web-wasm.company_catalog_view",
        );
        if let Err(cleanup_error) = session.shared_ingress().close() {
            failure.cause = Some(Box::new(FailureCause {
                code: "CATALOG_INGRESS_CLOSE_FAILED",
                message: "目录故障后关闭 ingress 失败".to_owned(),
                cause: failure_cause(&cleanup_error),
            }));
        }
        failure.context.tick = Some(session.tick());
        failure.context.seq = Some(session.seq());
        failure.context.day = Some(session.day());
        Box::new(failure)
    })
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
pub fn query_stock_history(handle: u32, code: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let history = session
            .query_stock_history(engine::AccountId(0), &engine::StockId(code))
            .map_err(session_error_to_js)?;
        public_dto_to_js(&history)
    })
}

/// B2 查询面（W3 接线）：保留历史分页（query 已为数值 StockId wire 形态——
/// 主线程 host 归一层完成 code→id 翻译，ADR-0041）。
#[wasm_bindgen]
pub fn market_history(handle: u32, query: JsValue) -> Result<JsValue, JsValue> {
    let request: MarketHistoryRequest = serde_wasm_bindgen::from_value(query)?;
    with_session(handle, |session| {
        let page = session
            .query_market_history_for(AccountId(0), &request)
            .map_err(session_error_to_js)?;
        public_dto_to_js(&page)
    })
}

/// B2 查询面（W3 接线）：当日分钟K。
#[wasm_bindgen]
pub fn current_minute_history(handle: u32, query: JsValue) -> Result<JsValue, JsValue> {
    let request: CurrentMinuteHistoryRequest = serde_wasm_bindgen::from_value(query)?;
    with_session(handle, |session| {
        let response = session
            .query_current_minute_history_for(AccountId(0), &request)
            .map_err(session_error_to_js)?;
        public_dto_to_js(&response)
    })
}

/// B2 查询面（W3 接线）：个人交割历史分页。
#[wasm_bindgen]
pub fn personal_trade_history(handle: u32, query: JsValue) -> Result<JsValue, JsValue> {
    let request: PersonalTradeHistoryRequest = serde_wasm_bindgen::from_value(query)?;
    with_session(handle, |session| {
        let page = session
            .query_personal_trade_history(AccountId(0), request)
            .map_err(session_error_to_js)?;
        public_dto_to_js(&page)
    })
}

/// B2 补面（W3 接线）：本人交割单游标分页（降序、排他游标、100 条页）。
#[wasm_bindgen]
pub fn personal_trade_confirmations(
    handle: u32,
    before_receipt: Option<u64>,
) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let page = session.personal_trade_confirmations_page(AccountId(0), before_receipt);
        public_dto_to_js(&page)
    })
}

/// W3 指标源：Rust 分时均价单点（纯函数——u128 wire 域直算，不经 u64
/// 会话域转一道，杜绝静默截断）。
#[wasm_bindgen]
pub fn calculate_intraday_average(
    turnover_cents: String,
    trade_count: u64,
    volume_shares: u64,
) -> Result<JsValue, JsValue> {
    let turnover_cents = engine::parse_turnover_cents(&turnover_cents)
        .map_err(|error| JsValue::from_str(&format!("turnover_cents 无效：{error}")))?;
    let input = engine::IntradayAverageInput {
        turnover_cents,
        trade_count,
        volume_shares,
    };
    let average = engine::calculate_intraday_average_from_input(&input)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    to_js(&average)
}

/// W3 指标源：Rust 分时均价曲线（批量纯函数——单次 FFI 跨界）。
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
pub fn initial_allocation(handle: u32) -> Result<JsValue, JsValue> {
    with_session(handle, |session| {
        let allocation = session.initial_allocation().map_err(session_error_to_js)?;
        public_dto_to_js(&allocation)
    })
}

#[wasm_bindgen]
pub fn calculate_indicators(prices: Vec<f64>, candles: JsValue) -> Result<JsValue, JsValue> {
    let candles: Vec<engine::indicators::OhlcBar> = serde_wasm_bindgen::from_value(candles)?;
    let indicators = engine::indicators::calculate_indicators(&prices, &candles, None)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    to_js(&indicators)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HostCapabilities {
    npc_decision_diagnostics: bool,
    /// W3 指标源能力面（B2 补面后 VWAP 纯函数可算——恒 true，Rust 侧无
    /// 构建形态门）。
    indicator_capabilities: IndicatorCapabilities,
    /// W3 个人交割历史查询能力（B2 查询面已接）。
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
            // 1303：ingress token 已关闭或不存在；ctx[0]=token。
            SessionError::ResourceLimit(engine::error::ValidationFault::new(
                1303,
                vec![u64::from(token)],
            ))
        })
    })
    .map_err(session_error_to_js)?;
    service
        .enqueue_player_intent(AccountId(0), intent)
        .map_err(session_error_to_js)
}

/// 销毁会话（释放内存）。
#[wasm_bindgen]
pub fn drop_session(handle: u32) -> Result<(), JsValue> {
    REGISTRY
        .with(|r| r.borrow_mut().remove(handle))
        .map_err(session_error_to_js)
}

/// 消费指定 session 的 Rust owner；不证明同 memory 其他 session 或 child 已退出。
#[wasm_bindgen]
pub fn shutdown_session(handle: u32) -> Result<JsValue, JsValue> {
    match REGISTRY.with(|registry| registry.borrow_mut().shutdown(handle)) {
        Ok(report) => serde_wasm_bindgen::to_value(&shutdown::SessionShutdownReply {
            handle,
            result: report,
        })
        .map_err(|error| JsValue::from_str(&format!("shutdown 成功事实序列化失败：{error}"))),
        Err(failure) => {
            let wire = serde_wasm_bindgen::to_value(&shutdown::SessionShutdownReply {
                handle,
                result: &failure,
            })
            .map_err(|error| {
                JsValue::from_str(&format!(
                    "shutdown 失败事实序列化失败：{error}；原错误={failure:?}"
                ))
            })?;
            Err(wire)
        }
    }
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
        .map_err(host_session_init_error_to_js)
}

#[wasm_bindgen]
pub fn restore_json(save_json: String) -> Result<u32, JsValue> {
    let slot: SaveSlot =
        serde_json::from_str(&save_json).map_err(|error| JsValue::from_str(&error.to_string()))?;
    REGISTRY
        .with(|registry| registry.borrow_mut().restore(&slot))
        .map_err(host_session_init_error_to_js)
}

/// 消费初始化时已解码并预检的唯一完整存档。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn restore_prepared() -> Result<u32, JsValue> {
    let slot = PREPARED_RESTORE
        .with(|prepared| prepared.borrow_mut().take())
        .map_err(JsValue::from_str)?;
    REGISTRY
        .with(|registry| registry.borrow_mut().restore(&slot))
        .map_err(host_session_init_error_to_js)
}

/// 取消尚未消费的初始恢复并释放存档。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn clear_prepared_restore() {
    PREPARED_RESTORE.with(|prepared| prepared.borrow_mut().clear());
}

// ---------------------------------------------------------------------------
// R4a 阶段 2：wasm 线程门面槽协议导出面（设计 §12-14）。
//
// 仅 wasm32 编译（native 上 facade 运行时不存在；宿主 bootstrap 在 substrate
// 就绪后调 facade_slot_init，子 Worker bootstrap 调 facade_child_trampoline）。
// 当前宿主尚未装配 supervisor/子 Worker/bootstrap，spawn 不可达，本导出面
// 是宿主装配的接口锚，不改变既有运行时行为（阶段边界见 agents 台账）。
// ---------------------------------------------------------------------------

/// facade 运行时初始化：容量 = 2×(G+N+2)（宿主按实际并行参数传入，不写死）。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn facade_slot_init(capacity: u32) -> Result<(), JsValue> {
    // engine 单线程期先锚定共享 ORIGIN，之后才允许 child/ingress 首次取时。
    let _ = engine::clock::Instant::now();
    engine::thread_facade::init_wasm_facade(capacity).map_err(JsValue::from_str)
}

#[cfg(any(target_family = "wasm", test))]
fn facade_slot_capacity_for_topology(groups: usize, desks: usize) -> Result<u32, &'static str> {
    let groups = u32::try_from(groups).map_err(|_| "facade topology 的 G 超出 u32 范围")?;
    let desks = u32::try_from(desks).map_err(|_| "facade topology 的 N 超出 u32 范围")?;
    let capacity = groups
        .checked_add(desks)
        .and_then(|count| count.checked_add(2))
        .and_then(|count| count.checked_mul(2))
        .ok_or("facade topology 容量计算超出 u32 范围")?;
    if capacity > engine::thread_facade::FACADE_SLOT_MAX_CAPACITY {
        return Err("facade topology 容量超出既有 arena 的 2^20 slot 上限");
    }
    Ok(capacity)
}

#[cfg(any(target_family = "wasm", test))]
#[derive(Debug)]
enum RestoreFacadeInitError {
    Decode(serde_json::Error),
    UnsupportedRuntimeSyncPolicy(
        #[cfg_attr(
            all(test, not(target_family = "wasm")),
            expect(
                dead_code,
                reason = "native test 不编译 WASM 恢复导出；该载荷由原 WASM 错误映射读取"
            )
        )]
        UnsupportedRuntimeSyncPolicy,
    ),
    Topology(SessionError),
    Capacity(
        #[cfg_attr(
            all(test, not(target_family = "wasm")),
            expect(
                dead_code,
                reason = "native test 不编译 WASM 恢复导出；该载荷由原 WASM 错误映射读取"
            )
        )]
        &'static str,
    ),
}

/// 按完整 SaveSlot JSON 校验构造前缀并计算 slot 容量；不分配 arena 或完成恢复。
#[cfg(test)]
fn facade_slot_capacity_for_restore_json(save_json: &str) -> Result<u32, RestoreFacadeInitError> {
    Ok(decode_restore_and_capacity(save_json)?.1)
}

#[cfg(any(target_family = "wasm", test))]
fn decode_restore_and_capacity(save_json: &str) -> Result<(SaveSlot, u32), RestoreFacadeInitError> {
    let save: SaveSlot = serde_json::from_str(save_json).map_err(RestoreFacadeInitError::Decode)?;
    require_wasm_runtime_sync_policy(save.setup.runtime_sync_policy)
        .map_err(RestoreFacadeInitError::UnsupportedRuntimeSyncPolicy)?;
    let (groups, desks) = save
        .restore_runtime_topology()
        .map_err(RestoreFacadeInitError::Topology)?;
    let capacity = facade_slot_capacity_for_topology(groups, desks)
        .map_err(RestoreFacadeInitError::Capacity)?;
    Ok((save, capacity))
}

#[cfg(any(target_family = "wasm", test))]
#[derive(Default)]
struct PreparedRestore {
    slot: Option<SaveSlot>,
}

#[cfg(any(target_family = "wasm", test))]
impl PreparedRestore {
    fn prepare(
        &mut self,
        json: &str,
        initialize: impl FnOnce(u32) -> Result<(), &'static str>,
    ) -> Result<u32, RestoreFacadeInitError> {
        if self.slot.is_some() {
            return Err(RestoreFacadeInitError::Capacity("已存在待消费的恢复档"));
        }
        let (slot, capacity) = decode_restore_and_capacity(json)?;
        initialize(capacity).map_err(RestoreFacadeInitError::Capacity)?;
        self.slot = Some(slot);
        Ok(capacity)
    }
    fn take(&mut self) -> Result<SaveSlot, &'static str> {
        self.slot.take().ok_or("没有待消费的恢复档")
    }
    fn clear(&mut self) {
        self.slot = None;
    }
}

#[cfg(target_family = "wasm")]
thread_local! { static PREPARED_RESTORE: RefCell<PreparedRestore> = RefCell::new(PreparedRestore::default()); }

/// 仅按经过完整校验的默认 new setup 初始化 facade 槽；不创建 Session 或 World。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn facade_slot_init_for_setup(setup: JsValue) -> Result<(), JsValue> {
    let setup: SessionSetup = serde_wasm_bindgen::from_value(setup)?;
    require_wasm_runtime_sync_policy(setup.runtime_sync_policy)
        .map_err(|error| JsValue::from_str(&error.to_string()))?;
    let (groups, desks) = setup
        .default_new_runtime_topology()
        .map_err(session_error_to_js)?;
    let capacity = facade_slot_capacity_for_topology(groups, desks).map_err(JsValue::from_str)?;
    facade_slot_init(capacity)
}

/// 仅按完整存档的构造前缀初始化 slot；不创建或登记会话，成功不表示完整恢复成功。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn facade_slot_init_for_restore_json(save_json: String) -> Result<(), JsValue> {
    let capacity = PREPARED_RESTORE
        .with(|prepared| prepared.borrow_mut().prepare(&save_json, |_| Ok(())))
        .map_err(|error| match error {
            RestoreFacadeInitError::Decode(error) => JsValue::from_str(&error.to_string()),
            RestoreFacadeInitError::UnsupportedRuntimeSyncPolicy(error) => {
                JsValue::from_str(&error.to_string())
            }
            RestoreFacadeInitError::Topology(error) => session_error_to_js(error),
            RestoreFacadeInitError::Capacity(error) => JsValue::from_str(error),
        })?;
    if let Err(error) = facade_slot_init(capacity) {
        clear_prepared_restore();
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod session_fixture;

#[cfg(test)]
mod facade_restore_json_tests;

#[cfg(test)]
mod facade_slot_capacity_tests {
    use super::facade_slot_capacity_for_topology;

    #[test]
    fn facade_slot_capacity_matches_default_new_formula() {
        for (desks, expected) in [(1, 8), (3, 12)] {
            assert_eq!(facade_slot_capacity_for_topology(1, desks), Ok(expected));
        }
    }

    #[test]
    fn facade_slot_capacity_accepts_existing_arena_maximum_without_allocating() {
        assert_eq!(facade_slot_capacity_for_topology(1, 524_285), Ok(1 << 20));
    }

    #[test]
    fn facade_slot_capacity_rejects_first_value_above_existing_arena_maximum() {
        assert!(facade_slot_capacity_for_topology(1, 524_286).is_err());
    }

    #[test]
    fn facade_slot_capacity_rejects_addition_and_multiplication_overflow() {
        assert!(facade_slot_capacity_for_topology(u32::MAX as usize, 1).is_err());
        assert!(facade_slot_capacity_for_topology((u32::MAX / 2) as usize, 0).is_err());
    }

    #[cfg(target_pointer_width = "64")]
    #[test]
    fn facade_slot_capacity_rejects_narrowing_before_arithmetic() {
        let outside_u32 = u32::MAX as usize + 1;
        assert!(facade_slot_capacity_for_topology(outside_u32, 1).is_err());
        assert!(facade_slot_capacity_for_topology(1, outside_u32).is_err());
    }
}

#[cfg(target_family = "wasm")]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct FacadeSlotLayout {
    base_addr: u32,
    header_words: u32,
    slot_stride: u32,
    join_stride: u32,
    capacity: u32,
    gen_bits: u32,
}

/// 共享字区布局（宿主建 Int32Array 视图的锚；wasm32 地址为 u32 域）。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn facade_slot_layout() -> Result<JsValue, JsValue> {
    let info = engine::thread_facade::facade_layout()
        .map_err(|error| JsValue::from_str(&format!("facade_slot_layout 失败：{error}")))?;
    to_js(&FacadeSlotLayout {
        base_addr: info.base_addr,
        header_words: info.header_words,
        slot_stride: info.slot_stride,
        join_stride: info.join_stride,
        capacity: info.capacity,
        gen_bits: info.gen_bits,
    })
}

/// 子 Worker trampoline 入口（W1-W3）：0=Ran、1=Superseded；协议违约 panic
/// ⇒ trap ⇒ 宿主 D5（§8.2.2）。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn facade_child_trampoline(slot: u32, gen: u32, worker_id: u32) -> u32 {
    engine::thread_facade::child_trampoline(slot, gen, worker_id)
}

/// wasm 收件显式一次初始化：四参数单位均为微秒；无默认策略，不允许同 memory 重置。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn init_wasm_wait(
    spin_us: u32,
    quiet_us: u32,
    fine_us: u32,
    coarse_us: u32,
) -> Result<(), JsValue> {
    engine::runtime::watchdog::init_wasm_wait(spin_us, quiet_us, fine_us, coarse_us)
        .map_err(|error| JsValue::from_str(&error))
}

/// 返回同一 memory 的单向宿主 abort 字布局；只允许 Uint8Array 原子写入 1，bytes 必须为 1。
#[cfg(target_family = "wasm")]
#[wasm_bindgen]
pub fn wasm_wait_abort_layout() -> Result<JsValue, JsValue> {
    let layout = engine::runtime::watchdog::wasm_wait_abort_layout()
        .map_err(|error| JsValue::from_str(&error))?;
    serde_wasm_bindgen::to_value(&layout)
        .map_err(|error| JsValue::from_str(&format!("wasm abort 布局序列化失败：{error}")))
}

// ---------------------------------------------------------------------------
// 验证专用入口（feature = verification-harness；正式产物零暴露）。
//
// 正式产品里没人调用 `thread_facade::Builder::spawn`（wasm 运行时仍是串行
// 管线），整条请求者链路会被 LTO 消除。真实双 Worker 闭环验证需要它存活，
// 故只在验证 feature 下导出。构建方式见 agents/wasm-thread-r4a/phase2-ledger.md。
// ---------------------------------------------------------------------------

/// 验证：保持旧入口可重复 spawn/join 的常量闭包合同。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[wasm_bindgen]
pub fn verification_facade_spawn(ack_timeout_ms: u32) -> Result<u32, JsValue> {
    let handle = engine::thread_facade::Builder::new()
        .name("verification-spawn".to_owned())
        .spawn(|| 0x5A5Au32)
        .map_err(|error| JsValue::from_str(&format!("verification_facade_spawn 失败：{error}")))?;
    // JoinHandle 必须存活到 join；返回 join 槽索引供宿主侧 join。
    let index = facade_join_index_of(&handle);
    FACADE_VERIFY_HANDLES.with(|slot| {
        let mut store = slot.borrow_mut();
        if store.is_some() {
            return Err(JsValue::from_str(
                "verification_facade_spawn：已有在途句柄（验证入口一次只允许一个）",
            ));
        }
        *store = Some(handle);
        let _ = ack_timeout_ms; // 预留：门面当前固定 SPAWN_ACK_TIMEOUT_MS
        Ok(index)
    })
}

/// 验证：发起一次槽协议 spawn（请求者侧 H1-H5），返回 join 句柄索引。
/// 闭包忠实执行一次并返回常量，用于验证 W1-W3 与 join 取回。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[wasm_bindgen]
pub fn verification_facade_spawn_probed(ack_timeout_ms: u32) -> Result<u32, JsValue> {
    verification_begin_probe()?;
    verification_capture_probe(0);
    let handle = engine::thread_facade::Builder::new()
        .name("verification-spawn".to_owned())
        .spawn(|| {
            verification_capture_probe(1);
            0x5A5Au32
        })
        .map_err(|error| JsValue::from_str(&format!("verification_facade_spawn 失败：{error}")))?;
    // JoinHandle 必须存活到 join；返回 join 槽索引供宿主侧 join。
    let index = facade_join_index_of(&handle);
    FACADE_VERIFY_HANDLES.with(|slot| {
        let mut store = slot.borrow_mut();
        if store.is_some() {
            return Err(JsValue::from_str(
                "verification_facade_spawn：已有在途句柄（验证入口一次只允许一个）",
            ));
        }
        *store = Some(handle);
        let _ = ack_timeout_ms; // 预留：门面当前固定 SPAWN_ACK_TIMEOUT_MS
        Ok(index)
    })
}

/// 验证：外层 child 同步 spawn/join grandchild；主线程必须独立接收嵌套 H5。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[wasm_bindgen]
pub fn verification_facade_spawn_nested() -> Result<u32, JsValue> {
    verification_begin_probe()?;
    verification_capture_probe(0);
    let handle = engine::thread_facade::Builder::new()
        .name("verification-outer".to_owned())
        .spawn(|| {
            verification_capture_probe(1);
            engine::thread_facade::Builder::new()
                .name("verification-inner".to_owned())
                .spawn(|| {
                    verification_capture_probe(2);
                    0x5A5Au32
                })
                .expect("verification nested spawn 失败")
                .join()
                .expect("verification nested join 失败")
        })
        .map_err(|error| JsValue::from_str(&format!("verification nested spawn 失败：{error}")))?;
    let index = facade_join_index_of(&handle);
    FACADE_VERIFY_HANDLES.with(|slot| {
        let mut store = slot.borrow_mut();
        if store.is_some() {
            return Err(JsValue::from_str("verification nested spawn：已有在途句柄"));
        }
        *store = Some(handle);
        Ok(index)
    })
}

// 固定三个角色记录，仅供验证读取；不形成运行时调度/业务状态或新生产 import。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
static FACADE_VERIFY_PROBES: [AtomicU32; 15] = [const { AtomicU32::new(0) }; 15];

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
static FACADE_VERIFY_PROBE_STARTED: AtomicU32 = AtomicU32::new(0);

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
fn verification_begin_probe() -> Result<(), JsValue> {
    FACADE_VERIFY_PROBE_STARTED
        .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
        .map(|_| ())
        .map_err(|_| JsValue::from_str("verification probe 每实例只允许一轮；请创建独立实例"))
}

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
thread_local! {
    static FACADE_VERIFY_TLS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
fn verification_capture_probe(index: usize) {
    assert!(
        index < 3,
        "verification probe 超出 engine/child/grandchild 范围"
    );
    let marker = 0x5A5Au32;
    let stack_addr = &marker as *const u32 as u32;
    let now = engine::clock::Instant::now();
    let monotonic = engine::clock::Instant::now() >= now;
    FACADE_VERIFY_TLS.with(|cell| {
        let previous = cell.replace(marker);
        for (offset, value) in [
            stack_addr,
            cell as *const _ as u32,
            previous,
            marker,
            u32::from(monotonic),
        ]
        .into_iter()
        .enumerate()
        {
            FACADE_VERIFY_PROBES[index * 5 + offset].store(value, Ordering::Release);
        }
    });
}

/// 验证：读取 engine/child/grandchild 的 stack/TLS 地址、TLS 前后值与 clock 单调标记。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[wasm_bindgen]
pub fn verification_facade_probe() -> Result<JsValue, JsValue> {
    let probes: Vec<u32> = FACADE_VERIFY_PROBES
        .iter()
        .map(|value| value.load(Ordering::Acquire))
        .collect();
    to_js(&probes)
}

/// 验证：join 上一次 verification_facade_spawn 并返回 u32 结果。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[wasm_bindgen]
pub fn verification_facade_join() -> Result<u32, JsValue> {
    let handle = FACADE_VERIFY_HANDLES.with(|slot| slot.borrow_mut().take());
    let handle = handle.ok_or_else(|| JsValue::from_str("verification_facade_join：无在途句柄"))?;
    handle
        .join()
        .map_err(|error| JsValue::from_str(&format!("verification_facade_join 失败：{error}")))
}

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
thread_local! {
    static FACADE_VERIFY_HANDLES: RefCell<Option<engine::thread_facade::JoinHandle<u32>>> =
        const { RefCell::new(None) };
}

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
fn facade_join_index_of(handle: &engine::thread_facade::JoinHandle<u32>) -> u32 {
    handle.join_index()
}

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
thread_local! {
    static WORLD_STOP_PROBE: RefCell<Option<Box<engine::runtime::world::WorldStopProbe>>> =
        const { RefCell::new(None) };
}

/// 验证专用：每个独立engine实例仅准备一个短World case及其共享阶段观测字。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[wasm_bindgen]
pub fn verification_world_stop_prepare(mode: u32) -> Result<u32, JsValue> {
    WORLD_STOP_PROBE.with(|store| {
        let mut store = store.borrow_mut();
        if store.is_some() {
            return Err(JsValue::from_str("World stop probe已准备，必须换独立实例"));
        }
        let probe = Box::new(
            engine::runtime::world::WorldStopProbe::new(mode)
                .map_err(|error| JsValue::from_str(&error))?,
        );
        let address = probe.address();
        *store = Some(probe);
        Ok(address)
    })
}

/// 验证专用：同一dispatcher调用真实World构造、永久停止及join；不创建GameSession。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[wasm_bindgen]
pub fn verification_world_stop_run(mode: u32) -> Result<String, JsValue> {
    WORLD_STOP_PROBE.with(|store| {
        let store = store.borrow();
        let probe = store
            .as_ref()
            .ok_or_else(|| JsValue::from_str("World stop probe尚未准备"))?;
        engine::runtime::world::verify_world_stop(mode, probe)
            .map_err(|error| JsValue::from_str(&error))
    })
}

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
thread_local! {
    static WATCHDOG_PROBE: RefCell<Option<std::sync::Arc<engine::runtime::watchdog_verification::WatchdogProbe>>> =
        const { RefCell::new(None) };
}

/// 验证专用：首次实际收件前准备共享阶段字，不替代生产 watchdog。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[wasm_bindgen]
pub fn verification_watchdog_prepare(mode: u32) -> Result<u32, JsValue> {
    WATCHDOG_PROBE.with(|store| {
        let mut store = store.borrow_mut();
        if store.is_some() {
            return Err(JsValue::from_str("watchdog probe 已准备，必须换独立实例"));
        }
        let probe = std::sync::Arc::new(
            engine::runtime::watchdog_verification::WatchdogProbe::new(mode)
                .map_err(|error| JsValue::from_str(&error))?,
        );
        engine::runtime::watchdog_verification::WatchdogProbe::install_panic_probe(&probe);
        let address = probe.address();
        *store = Some(probe);
        Ok(address)
    })
}

/// 验证专用：执行三种原生产入口中的一种，保留 FIFO 和排空后断连断言。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[wasm_bindgen]
pub fn verification_watchdog_run(mode: u32) -> Result<String, JsValue> {
    WATCHDOG_PROBE.with(|store| {
        let store = store.borrow();
        let probe = store
            .as_ref()
            .ok_or_else(|| JsValue::from_str("watchdog probe 尚未准备"))?;
        engine::runtime::watchdog_verification::verify_watchdog_recv(mode, probe)
            .map_err(|error| JsValue::from_str(&error))
    })
}

#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
thread_local! {
    static WATCHDOG_WAIT_PROBE: RefCell<Option<std::sync::Arc<engine::runtime::watchdog_wait_verification::WaitProbe>>> =
        const { RefCell::new(None) };
}

/// 验证专用：同一 memory 一次准备实际收件边界 probe，配置仍由调用者显式初始化。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[wasm_bindgen]
pub fn verification_watchdog_wait_prepare(mode: u32) -> Result<u32, JsValue> {
    WATCHDOG_WAIT_PROBE.with(|store| {
        let mut store = store.borrow_mut();
        if store.is_some() {
            return Err(JsValue::from_str("wait probe 已准备，必须换独立 memory"));
        }
        let probe = std::sync::Arc::new(
            engine::runtime::watchdog_wait_verification::WaitProbe::new(mode)
                .map_err(|error| JsValue::from_str(&error))?,
        );
        let address = probe.address();
        *store = Some(probe);
        Ok(address)
    })
}

/// 验证专用：执行实际相位/Hot/quiet/裸 recv 选路与返回前过滤；不模拟 clock/channel。
#[cfg(all(target_family = "wasm", feature = "verification-harness"))]
#[wasm_bindgen]
pub fn verification_watchdog_wait_boundary(mode: u32) -> Result<String, JsValue> {
    WATCHDOG_WAIT_PROBE.with(|store| {
        let probe = store
            .borrow()
            .as_ref()
            .cloned()
            .ok_or_else(|| JsValue::from_str("wait probe 尚未准备"))?;
        engine::runtime::watchdog_wait_verification::verify_wait_boundary(mode, probe)
            .map_err(|error| JsValue::from_str(&error))
    })
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

#[cfg(test)]
mod catalog_boundary_tests {
    use super::*;
    #[test]
    fn catalog_failure_keeps_numeric_company_context() {
        let error = engine::session::CompanyCatalogViewError::UnknownListedStock {
            company: engine::company::CompanyId(7),
        };
        let failure = HostFailure::catalog(
            &error,
            "company_catalog_view",
            "web-wasm.company_catalog_view",
        );
        assert_eq!(
            serde_json::to_value(failure).unwrap()["context"]["fault"],
            serde_json::json!({"code":3019,"context":[7]})
        );
    }
}

#[cfg(test)]
mod runtime_sync_policy_tests;
