//! apps/server 可执行入口。
//!
//! 职责单一：初始化 tracing → 取 router → 监听 0.0.0.0:3000 并 serve。
//! 路由装配在 `lib::app_router`，便于测试与后续阶段扩展。
//!
//! 错误处理（铁律二）：`axum::serve` 失败（端口被占等）通过 `?` 传播并以非零码退出，
//! 绝不静默吞错、绝不回退默认行为。

use server::{app_router, init_tracing};

const BIND_ADDR: &str = "0.0.0.0:3000";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    tracing::info!(addr = %BIND_ADDR, "server starting; GET /healthz -> ok");

    let listener = tokio::net::TcpListener::bind(BIND_ADDR).await?;
    axum::serve(listener, app_router().into_make_service()).await?;

    Ok(())
}
