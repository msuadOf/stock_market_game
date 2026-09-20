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

#[derive(Debug, PartialEq, Eq)]
enum BindAddrError {
    Empty,
    NotUnicode,
}

impl std::fmt::Display for BindAddrError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => {
                formatter.write_str("STOCK_MARKET_GAME_SERVER_BIND_ADDR 不能为空或仅包含空白字符")
            }
            Self::NotUnicode => {
                formatter.write_str("STOCK_MARKET_GAME_SERVER_BIND_ADDR 必须是有效 Unicode")
            }
        }
    }
}

impl std::error::Error for BindAddrError {}

fn resolve_bind_addr(value: Result<String, std::env::VarError>) -> Result<String, BindAddrError> {
    match value {
        Ok(address) if address.trim().is_empty() => Err(BindAddrError::Empty),
        Ok(address) => Ok(address),
        Err(std::env::VarError::NotPresent) => Ok(BIND_ADDR.to_owned()),
        Err(std::env::VarError::NotUnicode(_)) => Err(BindAddrError::NotUnicode),
    }
}

fn bind_addr() -> Result<String, BindAddrError> {
    resolve_bind_addr(std::env::var("STOCK_MARKET_GAME_SERVER_BIND_ADDR"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn missing_bind_address_uses_loopback_default() {
        assert_eq!(
            resolve_bind_addr(Err(std::env::VarError::NotPresent)).unwrap(),
            BIND_ADDR
        );
    }

    #[test]
    fn configured_bind_address_is_preserved() {
        assert_eq!(
            resolve_bind_addr(Ok("192.0.2.10:4000".to_owned())).unwrap(),
            "192.0.2.10:4000"
        );
    }

    #[test]
    fn empty_or_whitespace_bind_address_is_rejected() {
        for address in ["", "   ", "\t\n"] {
            assert!(matches!(
                resolve_bind_addr(Ok(address.to_owned())),
                Err(BindAddrError::Empty)
            ));
        }
    }

    #[test]
    fn non_unicode_bind_address_is_rejected() {
        assert!(matches!(
            resolve_bind_addr(Err(std::env::VarError::NotUnicode(OsString::from(
                "not-unicode"
            )))),
            Err(BindAddrError::NotUnicode)
        ));
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_tracing();
    let bind_addr = bind_addr()?;
    tracing::info!(addr = %bind_addr, "server starting; GET /healthz -> ok");

    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    axum::serve(listener, app_router().into_make_service()).await?;

    Ok(())
}
