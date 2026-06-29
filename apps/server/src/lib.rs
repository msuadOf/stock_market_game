//! apps/server —— Axum 后端 crate。
//!
//! 本文件提供可测试的 router 工厂（`app_router`）与 tracing 初始化（`init_tracing`），
//! 把"构建路由"与"启动服务"分离，便于集成测试在不绑定端口的前提下黑盒验证契约。
//!
//! 复用 engine（Rust-to-Rust 直接 rlib 依赖，不经 FFI/WASM）：
//!   engine::{GameSession, SessionSetup, Intent, Snapshot, Event, AccountId, SessionError}
//! 当前阶段（WS-5 脚手架）仅落地存活探针；多会话 actor、WS 桥接、命令/事件管道
//! 在后续阶段基于同一 router 叠加。
//!
//! 工程铁律：
//! - 不静默吞错：启动失败（端口绑定等）直接返回/传播 Result，不 return 默认值。
//! - 显式反馈：路由按 name 承诺，副作用（监听）显式发生在 main。

use axum::{routing::get, Router};
use tracing_subscriber::EnvFilter;

/// 存活探针：返回纯文本 "ok"。
///
/// 仅用于 liveness，不读取任何外部状态；返回 `&'static str` 即可，
/// Axum 会以 `text/plain; charset=utf-8` 返回。
async fn healthz() -> &'static str {
    "ok"
}

/// 构建应用路由（无副作用，可重复调用、可测试）。
///
/// 把路由装配从 `main` 中抽出来：集成测试用 `oneshot` 直接打这个 router，
/// 而 `main` 仅负责 `init_tracing` + `serve`。
pub fn app_router() -> Router {
    Router::new().route("/healthz", get(healthz))
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
