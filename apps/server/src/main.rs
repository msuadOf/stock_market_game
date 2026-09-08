//! apps/server 可执行入口。
//!
//! 职责单一：初始化 tracing → 取 router → 默认仅监听本机回环地址并 serve。
//! 路由装配在 `lib::app_router`，便于测试与后续阶段扩展。
//!
//! 错误处理（铁律二）：`axum::serve` 失败（端口被占等）通过 `?` 传播并以非零码退出，
//! 绝不静默吞错、绝不回退默认行为。

use server::{app_router, init_tracing};

// 当前 API 尚未实现会话级鉴权；默认暴露到所有网卡会让同网段访问者读写任意会话。
// 远程部署必须先完成鉴权与 origin 白名单，再通过受审配置显式开放监听地址。
const BIND_ADDR: &str = "127.0.0.1:3000";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    tracing::info!(addr = %BIND_ADDR, "server starting; GET /healthz -> ok");

    let listener = tokio::net::TcpListener::bind(BIND_ADDR).await?;
    axum::serve(listener, app_router().into_make_service()).await?;

    Ok(())
}
