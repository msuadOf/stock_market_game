//! WS /ws 端到端集成测试（连真实绑端口的服务器，验证 on_upgrade 全链路）。
//!
//! 契约：
//! - 连接后**先**收到完整 Snapshot（JSON）对齐基线；
//! - 随后持续收到 Event[] JSON（各带 seq）；
//! - 缺 token / 未知 session → 握手失败（HTTP 错误状态，非 101）。

use std::time::Duration;

use futures_util::StreamExt;
use server::{app_router_with_manager, SessionManager};
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::http::Request as WsRequest;

/// 复用 api_contract 的合法 setup JSON（这里独立构造一份，避免跨测试文件依赖）。
fn sample_setup_json() -> serde_json::Value {
    serde_json::json!({
        "stocks": [{
            "code": "600101",
            "initial_price": 1000,
            "limit_pct": 0.10,
            "v_initial": 1000,
            "tick": 1,
            "float_shares": 0
        }],
        "npcs": {
            "retail_count": 2,
            "inst_count": 1,
            "hot_count": 1,
            "cash_per_npc": 10_000_000
        },
        "config": engine::GameConfig::proposed_defaults(),
        "v_params": { "long_run_mean": 1000, "mean_reversion": 0.5, "volatility": 0.0 },
        "strategy_params": {
            "retail": { "arrival_rate": 0.5, "order_size_mean": 100, "chase_prob": 0.2, "tick_cents": 1 },
            "inst":   { "margin": 0.05, "order_size": 200 },
            "hot":    { "lookback": 3, "trend_threshold": 0.02, "order_size": 150 }
        },
        "player_cash": 10_000_000,
        "ticks_per_day": 10,
        "history_len": 5,
        "t1_enabled": false,
        "float_allocation": "Random"
    })
}

/// 起一个绑临时端口的服务器，返回 (base_url, manager)。
async fn spawn_server(base_ms: u64) -> (String, SessionManager) {
    let manager = SessionManager::with_base_ms(base_ms);
    let app = app_router_with_manager(manager.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app.into_make_service()).await.unwrap();
    });
    (format!("http://{addr}"), manager)
}

/// 创建一个 session（经 HTTP /api/new），返回 session_id。
async fn create_session(_base_url: &str, manager: &SessionManager, seed: u64) -> String {
    // 直接用 manager 构造，避免再起 HTTP 客户端（manager 是同一实例）。
    let setup: engine::SessionSetup =
        serde_json::from_value(sample_setup_json()).unwrap();
    manager.new_session(setup, seed).unwrap()
}

#[tokio::test]
async fn ws_sends_baseline_snapshot_then_events() {
    // base_ms=20ms 让事件快速到达。
    let (base_url, manager) = spawn_server(20).await;
    let id = create_session(&base_url, &manager, 42).await;

    let ws_url = base_url.replace("http://", "ws://");
    let req = WsRequest::builder()
        .method("GET")
        .uri(format!("{ws_url}/ws?session_id={id}&token=test-token"))
        .header("Host", base_url.trim_start_matches("http://"))
        .header("Upgrade", "websocket")
        .header("Connection", "upgrade")
        .header("Sec-WebSocket-Key", generate_key())
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .unwrap();
    let (mut ws, _resp) = tokio_tungstenite::connect_async(req).await.expect("WS 握手应成功");

    // 1. 首条消息应是完整 Snapshot（JSON），含 markets/accounts。
    let first = tokio::time::timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("应在超时前收到首条消息")
        .expect("stream 不应立即结束")
        .expect("读消息不应出错");
    let text = first.into_text().expect("首条应为文本帧");
    let snap: serde_json::Value = serde_json::from_str(&text).expect("首条应为 Snapshot JSON");
    assert!(snap.get("markets").is_some(), "Snapshot 应含 markets: {snap}");
    assert!(snap.get("accounts").is_some(), "Snapshot 应含 accounts: {snap}");
    assert!(snap.get("seq").is_some(), "Snapshot 应含 seq");

    // 2. 随后应持续收到 Event JSON（各带 seq）。收若干条验证带 seq。
    let mut got_events_with_seq = 0;
    for _ in 0..20 {
        let msg = match tokio::time::timeout(Duration::from_secs(3), ws.next()).await {
            Ok(Some(Ok(m))) => m,
            _ => break,
        };
        if let Ok(t) = msg.into_text() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                // Event 经 serde 外部标记序列化：{"PriceTick":{"seq":6,...}}。
                // seq 嵌套在内层对象，故遍历顶层对象的值找 seq。
                let has_seq = v
                    .as_object()
                    .and_then(|o| o.values().next())
                    .and_then(|inner| inner.get("seq"))
                    .and_then(|s| s.as_u64())
                    .is_some();
                if has_seq {
                    got_events_with_seq += 1;
                    if got_events_with_seq >= 2 {
                        break;
                    }
                }
            }
        }
    }
    assert!(
        got_events_with_seq >= 1,
        "连接后应收到至少 1 条带 seq 的 Event，实际 {got_events_with_seq}"
    );
}

#[tokio::test]
async fn ws_rejects_unknown_session() {
    let (base_url, _manager) = spawn_server(1000).await;
    let ws_url = base_url.replace("http://", "ws://");

    let req = WsRequest::builder()
        .method("GET")
        .uri(format!("{ws_url}/ws?session_id=does-not-exist&token=t"))
        .header("Host", "127.0.0.1")
        .header("Upgrade", "websocket")
        .header("Connection", "upgrade")
        .header("Sec-WebSocket-Key", generate_key())
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .unwrap();
    // 未知 session → handler 返回 404（非 101）→ connect 应失败。
    let res = tokio_tungstenite::connect_async(req).await;
    assert!(res.is_err(), "未知 session 握手应失败（非 101 升级）");
}
