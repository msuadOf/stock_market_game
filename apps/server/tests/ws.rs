//! WS /ws 端到端集成测试（连真实绑端口的服务器，验证 on_upgrade 全链路）。
//!
//! 契约：
//! - 连接后**先**收到完整 Snapshot（JSON）对齐基线；
//! - 随后持续收到 Event[] JSON（各带 seq）；
//! - 缺 token / 未知 session → 握手失败（HTTP 错误状态，非 101）。

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use server::{app_router_with_manager, SessionManager};
use tokio_tungstenite::tungstenite::handshake::client::generate_key;
use tokio_tungstenite::tungstenite::http::Request as WsRequest;

/// 复用 api_contract 的合法 setup JSON（这里独立构造一份，避免跨测试文件依赖）。
fn sample_setup_json() -> serde_json::Value {
    serde_json::json!({
        "stocks": [{
            "code": "600101",
            "exchange": "Shanghai",
            "category": "MainBoard",
            "initial_price": 1000,
            "limit_pct": 0.10,
            "tick": 1,
            "total_shares": "10000000",
            "float_shares": 0
        }],
        "npcs": {
            "retail_count": 2,
            "inst_count": 1,
            "hot_count": 1,
            "retail_cash_median": 10_000_000
        },
        "config": engine::GameConfig::proposed_defaults(),
        "strategy_params": {
            "retail": { "arrival_rate": 0.5, "order_size_mean": 100, "chase_prob": 0.2, "tick_cents": 1 },
            "inst":   { "margin": 0.05, "order_size": 200 },
            "hot":    { "lookback": 3, "trend_threshold": 0.02, "order_size": 200 }
        },
        "ticks_per_day": 10,
        "auction_ticks": 0,
        "closing_auction_ticks": 0,
        "history_len": 5,
        "t1_enabled": true,
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
        axum::serve(listener, app.into_make_service())
            .await
            .unwrap();
    });
    (format!("http://{addr}"), manager)
}

/// 创建一个 session（经 HTTP /api/new），返回 session_id。
async fn create_session(_base_url: &str, manager: &SessionManager, seed: u64) -> String {
    // 直接用 manager 构造，避免再起 HTTP 客户端（manager 是同一实例）。
    let setup: engine::SessionSetup = serde_json::from_value(sample_setup_json()).unwrap();
    manager.new_session(setup, seed).unwrap()
}

#[tokio::test]
async fn ws_sends_baseline_snapshot_then_events() {
    // base_ms=20ms 让事件快速到达。
    let (base_url, manager) = spawn_server(20).await;
    let id = create_session(&base_url, &manager, 42).await;
    manager
        .lookup(&id)
        .unwrap()
        .set_running(true)
        .await
        .unwrap();

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
    let (mut ws, _resp) = tokio_tungstenite::connect_async(req)
        .await
        .expect("WS 握手应成功");

    // 1. 首条消息应是完整 Snapshot（JSON），含 markets/accounts。
    let first = tokio::time::timeout(Duration::from_secs(2), ws.next())
        .await
        .expect("应在超时前收到首条消息")
        .expect("stream 不应立即结束")
        .expect("读消息不应出错");
    let text = first.into_text().expect("首条应为文本帧");
    let snap: serde_json::Value = serde_json::from_str(&text).expect("首条应为 Snapshot JSON");
    assert!(
        snap.get("markets").is_some(),
        "Snapshot 应含 markets: {snap}"
    );
    assert!(
        snap.get("accounts").is_some(),
        "Snapshot 应含 accounts: {snap}"
    );
    assert!(snap.get("seq").is_some(), "Snapshot 应含 seq");
    assert_eq!(
        snap["daily_candles"]["600101"].as_array().map(Vec::len),
        Some(360),
        "WS 首帧应同步 Rust 生成的 360 日日 K"
    );

    // 2. 默认 push Publisher 以 60Hz+ 节拍发送压缩帧。
    let mut got_events_with_seq = 0;
    for _ in 0..20 {
        let msg = match tokio::time::timeout(Duration::from_secs(3), ws.next()).await {
            Ok(Some(Ok(m))) => m,
            _ => break,
        };
        if let Ok(t) = msg.into_text() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&t) {
                let events = v
                    .get("PublisherFrame")
                    .and_then(|u| u.get("events"))
                    .and_then(|e| e.as_array());
                if let Some(events) = events {
                    got_events_with_seq += events
                        .iter()
                        .filter(|event| {
                            event
                                .as_object()
                                .and_then(|o| o.values().next())
                                .and_then(|inner| inner.get("seq"))
                                .and_then(|s| s.as_u64())
                                .is_some()
                        })
                        .count();
                    if got_events_with_seq >= 1 {
                        break;
                    }
                }
            }
        }
    }
    assert!(
        got_events_with_seq >= 1,
        "连接后应收到至少 1 条带 seq 的 EngineUpdate 事件，实际 {got_events_with_seq}"
    );
}

#[tokio::test]
async fn pull_publisher_waits_for_client_and_then_returns_accumulated_frame() {
    let (base_url, manager) = spawn_server(20).await;
    let id = create_session(&base_url, &manager, 43).await;
    manager
        .lookup(&id)
        .unwrap()
        .set_running(true)
        .await
        .unwrap();
    let ws_url = base_url.replace("http://", "ws://");
    let req = WsRequest::builder()
        .method("GET")
        .uri(format!("{ws_url}/ws?session_id={id}&token=t&delivery=pull"))
        .header("Host", base_url.trim_start_matches("http://"))
        .header("Upgrade", "websocket")
        .header("Connection", "upgrade")
        .header("Sec-WebSocket-Key", generate_key())
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let _baseline = ws.next().await.unwrap().unwrap();

    assert!(
        tokio::time::timeout(Duration::from_millis(80), ws.next())
            .await
            .is_err(),
        "pull 模式在 GetFrame 前不得主动推行情帧"
    );
    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        r#"{"GetFrame":{}}"#.into(),
    ))
    .await
    .unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), ws.next())
        .await
        .unwrap()
        .unwrap()
        .unwrap()
        .into_text()
        .unwrap();
    let value: serde_json::Value = serde_json::from_str(&response).unwrap();
    assert!(value.get("PublisherFrame").is_some(), "实际响应：{value}");
}

#[tokio::test]
async fn gateway_reports_malformed_commands_and_queues_writes_explicitly() {
    let (base_url, manager) = spawn_server(1_000).await;
    let id = create_session(&base_url, &manager, 44).await;
    let ws_url = base_url.replace("http://", "ws://");
    let req = WsRequest::builder()
        .method("GET")
        .uri(format!("{ws_url}/ws?session_id={id}&token=t&delivery=pull"))
        .header("Host", base_url.trim_start_matches("http://"))
        .header("Upgrade", "websocket")
        .header("Connection", "upgrade")
        .header("Sec-WebSocket-Key", generate_key())
        .header("Sec-WebSocket-Version", "13")
        .body(())
        .unwrap();
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    let _baseline = ws.next().await.unwrap().unwrap();

    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        "not-json".into(),
    ))
    .await
    .unwrap();
    let error = ws.next().await.unwrap().unwrap().into_text().unwrap();
    let error: serde_json::Value = serde_json::from_str(&error).unwrap();
    assert_eq!(error["GatewayError"]["code"], "INVALID_CLIENT_COMMAND");

    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        serde_json::json!({
            "SubmitIntent": {
                "request_id": 7,
                "intent": { "PlaceLimit": { "code": "600101", "side": "Buy", "price": 1000, "qty": 100 } }
            }
        }).to_string(),
    )).await.unwrap();
    let queued = ws.next().await.unwrap().unwrap().into_text().unwrap();
    let queued: serde_json::Value = serde_json::from_str(&queued).unwrap();
    assert_eq!(queued["CommandQueued"]["request_id"], 7);
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

/// 手工性能探针：release 模式下分别跑 push / pull，并输出服务端权威实际倍率。
/// 不设置机器相关的胜负阈值；结果用于同一台机器、同一提交上的相对比较。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "手工性能探针：cargo test -p server --release --test ws publisher_modes_report_actual_speed -- --ignored --nocapture"]
async fn publisher_modes_report_actual_speed() {
    async fn measure(mode: &str) -> f64 {
        let (base_url, manager) = spawn_server(1_000).await;
        let id = create_session(&base_url, &manager, 100).await;
        let handles = manager.lookup(&id).unwrap();
        handles.set_speed(f64::INFINITY).await.unwrap();
        handles.set_running(true).await.unwrap();
        let ws_url = base_url.replace("http://", "ws://");
        let req = WsRequest::builder()
            .method("GET")
            .uri(format!(
                "{ws_url}/ws?session_id={id}&token=perf&delivery={mode}"
            ))
            .header("Host", base_url.trim_start_matches("http://"))
            .header("Upgrade", "websocket")
            .header("Connection", "upgrade")
            .header("Sec-WebSocket-Key", generate_key())
            .header("Sec-WebSocket-Version", "13")
            .body(())
            .unwrap();
        let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
        let _baseline = ws.next().await.unwrap().unwrap();
        let deadline = tokio::time::Instant::now() + Duration::from_millis(1_500);
        while tokio::time::Instant::now() < deadline {
            if mode == "pull" {
                ws.send(tokio_tungstenite::tungstenite::Message::Text(
                    r#"{"GetFrame":{}}"#.into(),
                ))
                .await
                .unwrap();
            }
            let _ = tokio::time::timeout(Duration::from_millis(100), ws.next()).await;
        }
        let metrics = handles.speed_metrics().await.unwrap();
        handles.shutdown().await.unwrap();
        metrics.actual_multiplier.expect("性能窗口应产生实际倍率")
    }

    let push = measure("push").await;
    let pull = measure("pull").await;
    assert!(push.is_finite() && push > 0.0);
    assert!(pull.is_finite() && pull > 0.0);
    println!(
        "Publisher actual speed: push={push:.1}x, pull={pull:.1}x, pull/push={:.3}",
        pull / push
    );
}
