//! apps/server —— Axum 后端 crate。
//!
//! 本文件提供可测试的 router 工厂（`app_router` / `app_router_with_manager`）与 tracing 初始化，
//! 把"构建路由"与"启动服务"分离，便于集成测试在不绑定端口的前提下黑盒验证契约。
//!
//! 复用 engine（Rust-to-Rust 直接 rlib 依赖，不经 FFI/WASM）：
//!   engine::{GameSession, SessionSetup, Intent, Snapshot, Event, AccountId, SessionError}
//!
//! 多线程模型（ADR-0005 §5，actor-per-session、无锁、契合 engine `Send`）：
//! - `actor::SessionManager` 持 `DashMap<session_id, Arc<SessionHandles>>`，每 session 一个
//!   tokio task 独占 `GameSession`，命令经 mpsc、事件经 broadcast。
//! - 路由层（`routes`）把 HTTP/WS 请求翻译成 `SessionCommand`，经 `SessionHandles` 投递。
//!
//! 工程铁律：
//! - 不静默吞错：未知 session → 404；非法 body → 400；engine 失败透传文案（见 `routes`）。
//! - 显式反馈：路由按 name 承诺，副作用（监听）显式发生在 main。

pub mod actor;
pub mod routes;

pub use actor::{SendCommandError, SessionHandles, SessionManager};
pub use routes::AppState;

use axum::routing::{get, post};
use axum::Router;
use tracing_subscriber::EnvFilter;

/// 存活探针：返回纯文本 "ok"。
async fn healthz() -> &'static str {
    "ok"
}

/// 构建应用路由（无副作用，可重复调用、可测试）。
///
/// 每次调用构造一个新的 `SessionManager`（生产用 `main` 调一次即可）。
/// 集成测试若需跨请求共享同一 session，请用 `app_router_with_manager`。
pub fn app_router() -> Router {
    app_router_with_state(AppState { manager: SessionManager::default() })
}

/// 用给定 `SessionManager` 构建路由（跨请求共享 session 的测试场景）。
pub fn app_router_with_manager(manager: SessionManager) -> Router {
    app_router_with_state(AppState { manager })
}

/// 内部：由 `AppState` 装配完整路由树。
fn app_router_with_state(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .route("/api/new", post(routes::api_new))
        .route("/api/intent", post(routes::api_intent))
        .route("/api/snapshot", get(routes::api_snapshot))
        .route("/api/speed", post(routes::api_speed))
        .route("/ws", get(routes::ws_handler))
        .with_state(state)
}

/// 初始化 tracing：默认 INFO 级别，允许用 `RUST_LOG` 覆盖。
///
/// 失败不静默：`try_init` 返回 Err 时说明已被初始化（重复 init 场景），
/// 此处用 `ok()` 抑制的是"已初始化"这一良性情况；真正的 panic 仍由 subscriber 捕获。
/// 注意：本函数预期在 main 早期调用一次。
pub fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter)
        // 重复初始化时 try_init 返回 Err，属良性（如测试中多次构造），不静默吞业务错误。
        .try_init()
        .ok();
}
