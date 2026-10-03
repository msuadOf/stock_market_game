//! WS /ws 端到端集成测试（连真实绑端口的服务器，验证 on_upgrade 全链路）。
//!
//! 契约：
//! - 连接后先收到显式 public baseline（JSON）对齐基线；
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
            "retail": { "arrival_rate": 0.5, "order_size_mean": 100, "chase_prob": 0.2 },
            "inst":   { "margin": 0.05, "order_size": 200 },
            "hot":    { "lookback": 3, "trend_threshold": 0.02, "order_size": 200 }
        },
        "ticks_per_day": 10,
        "auction_ticks": 0,
        "closing_auction_ticks": 0,
        "history_len": 5,
        "t1_enabled": true,
        "float_allocation": "Random",
        "simulation_policy_id": engine::SIMULATION_POLICY_ID_V2
    })
}

/// 拥有同一次启动的 listener task、注册表与会话清理清单。
struct ServerFixture {
    base_url: String,
    manager: SessionManager,
    server_task: tokio::task::JoinHandle<std::io::Result<()>>,
    session_ids: std::sync::Mutex<Vec<String>>,
}

impl ServerFixture {
    async fn start(base_ms: u64) -> Self {
        let manager = SessionManager::with_base_ms(base_ms);
        let app = app_router_with_manager(manager.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server_task =
            tokio::spawn(async move { axum::serve(listener, app.into_make_service()).await });
        Self {
            base_url: format!("http://{addr}"),
            manager,
            server_task,
            session_ids: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn session(&self, seed: u64) -> (String, std::sync::Arc<server::SessionHandles>) {
        let setup = serde_json::from_value(sample_setup_json()).expect("WS fixture setup 应合法");
        self.session_with_setup(setup, seed)
    }

    fn session_with_setup(
        &self,
        setup: engine::SessionSetup,
        seed: u64,
    ) -> (String, std::sync::Arc<server::SessionHandles>) {
        let id = self
            .manager
            .new_session(setup, seed)
            .expect("WS fixture actor 应启动");
        self.session_ids
            .lock()
            .expect("WS fixture session 清单不应 poison")
            .push(id.clone());
        let handles = self.manager.lookup(&id).expect("WS fixture actor 应已注册");
        (id, handles)
    }

    fn ws_url(&self) -> String {
        self.base_url.replace("http://", "ws://")
    }

    async fn shutdown(self) {
        // 先停止接收新连接，再移除并确认每个已创建的 actor 停止。
        self.server_task.abort();
        let server_result = self.server_task.await;
        let mut failures = Vec::new();
        for id in self
            .session_ids
            .into_inner()
            .expect("WS fixture session 清单不应 poison")
        {
            match self.manager.remove(&id) {
                Some(handles) => match handles.shutdown().await {
                    Ok(()) | Err(server::SendCommandError::ActorGone) => {}
                    Err(error) => failures.push(format!("session {id} 清理失败：{error}")),
                },
                None => failures.push(format!("session {id} 清理时不在注册表中")),
            }
        }
        match server_result {
            Ok(Ok(())) => {}
            Err(error) if error.is_cancelled() => {}
            other => failures.push(format!("WS fixture listener task 失败：{other:?}")),
        }
        assert!(failures.is_empty(), "{}", failures.join("；"));
    }

    async fn run<F, Fut, T>(base_ms: u64, test: F) -> T
    where
        F: FnOnce(std::sync::Arc<Self>) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let fixture = std::sync::Arc::new(Self::start(base_ms).await);
        let test_fixture = fixture.clone();
        // 即使断言 panic，外层仍执行显式异步清理，然后恢复原 panic。
        let result = tokio::spawn(async move { test(test_fixture).await }).await;
        let fixture = match std::sync::Arc::try_unwrap(fixture) {
            Ok(fixture) => fixture,
            Err(_) => panic!("WS fixture 不应逃逸测试生命周期"),
        };
        fixture.shutdown().await;
        match result {
            Ok(value) => value,
            Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
            Err(error) => panic!("WS fixture 测试任务被取消：{error}"),
        }
    }
}

#[tokio::test]
async fn ws_sends_baseline_snapshot_then_events() {
    // base_ms=20ms 让事件快速到达。
    ServerFixture::run(20, |fixture| async move {
        let base_url = &fixture.base_url;
        let (id, handles) = fixture.session(42);
        let token = handles.session_token.clone();
        handles.set_running(true).await.unwrap();

        let ws_url = fixture.ws_url();
        let req = WsRequest::builder()
            .method("GET")
            .uri(format!("{ws_url}/ws?session_id={id}"))
            .header("authorization", format!("Bearer {token}"))
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
        let baseline: serde_json::Value =
            serde_json::from_str(&text).expect("首条应为 baseline JSON");
        let snap = &baseline["Baseline"]["snapshot"];
        assert!(
            snap.get("markets").is_some(),
            "Snapshot 应含 markets: {snap}"
        );
        assert!(
            snap.get("accounts").is_some(),
            "Snapshot 应含 accounts: {snap}"
        );
        assert!(snap.get("seq").is_some(), "Snapshot 应含 seq");
        assert!(
            baseline["Baseline"]["civil_date"].is_string(),
            "baseline 应含当前自然日"
        );
        assert!(
            baseline["Baseline"]["public_revision"].is_u64(),
            "baseline 应含公开修订号"
        );
        assert!(
            baseline["Baseline"]["public_report_ids"].is_array(),
            "baseline 应含公开报告索引"
        );
        assert!(baseline["Baseline"].get("public_library").is_none());
        assert!(baseline["Baseline"].get("journal").is_none());
        assert!(baseline["Baseline"].get("npc_information_state").is_none());
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
                        .and_then(|u| u.get("update"))
                        .and_then(|u| u.get("TickBatch"))
                        .and_then(|u| u.get("frames"))
                        .and_then(|u| u.as_array())
                        .and_then(|frames| frames.first())
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
    })
    .await;
}

#[tokio::test]
async fn pull_publisher_waits_for_client_and_then_returns_accumulated_frame() {
    ServerFixture::run(20, |fixture| async move {
        let base_url = &fixture.base_url;
        let (id, handles) = fixture.session(43);
        let token = handles.session_token.clone();
        handles.set_running(true).await.unwrap();
        let ws_url = fixture.ws_url();
        let req = WsRequest::builder()
            .method("GET")
            .uri(format!("{ws_url}/ws?session_id={id}&delivery=pull"))
            .header("authorization", format!("Bearer {token}"))
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
    })
    .await;
}

#[tokio::test]
async fn gateway_reports_malformed_commands_and_queues_writes_explicitly() {
    ServerFixture::run(1_000, |fixture| async move {
    let base_url = &fixture.base_url;
    let (id, handles) = fixture.session(44);
    let token = handles.session_token.clone();
    let ws_url = fixture.ws_url();
    let req = WsRequest::builder()
        .method("GET")
        .uri(format!("{ws_url}/ws?session_id={id}&delivery=pull"))
        .header("authorization", format!("Bearer {token}"))
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
                "request_id": 6,
                "intent": { "PlaceLimit": { "code": "600101", "side": "Buy", "price": 1000, "qty": 100 } }
            }
        }).to_string(),
    )).await.unwrap();
    let old_price_error = ws.next().await.unwrap().unwrap().into_text().unwrap();
    let old_price_error: serde_json::Value = serde_json::from_str(&old_price_error).unwrap();
    assert_eq!(
        old_price_error["GatewayError"]["code"],
        "INVALID_CLIENT_COMMAND"
    );

    ws.send(tokio_tungstenite::tungstenite::Message::Text(
        serde_json::json!({
            "SubmitIntent": {
                "request_id": 7,
                "intent": { "PlaceLimit": { "code": "600101", "side": "Buy", "price": { "Fixed": 1000 }, "qty": 100 } }
            }
        }).to_string(),
    )).await.unwrap();
    let queued = ws.next().await.unwrap().unwrap().into_text().unwrap();
    let queued: serde_json::Value = serde_json::from_str(&queued).unwrap();
    assert_eq!(queued["CommandQueued"]["request_id"], 7);
    }).await;
}

#[tokio::test]
async fn ws_rejects_unknown_session() {
    ServerFixture::run(1000, |fixture| async move {
        let ws_url = fixture.ws_url();

        let req = WsRequest::builder()
            .method("GET")
            .uri(format!("{ws_url}/ws?session_id=does-not-exist"))
            .header("authorization", "Bearer t")
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
    })
    .await;
}

#[tokio::test]
async fn ws_rejects_missing_or_query_string_credentials() {
    ServerFixture::run(1_000, |fixture| async move {
        let base_url = &fixture.base_url;
        let (id, handles) = fixture.session(46);
        let token = handles.session_token.clone();
        let ws_url = fixture.ws_url();

        for request in [
            WsRequest::builder()
                .method("GET")
                .uri(format!("{ws_url}/ws?session_id={id}"))
                .header("Host", base_url.trim_start_matches("http://"))
                .header("Upgrade", "websocket")
                .header("Connection", "upgrade")
                .header("Sec-WebSocket-Key", generate_key())
                .header("Sec-WebSocket-Version", "13")
                .body(())
                .unwrap(),
            WsRequest::builder()
                .method("GET")
                .uri(format!("{ws_url}/ws?session_id={id}&token={token}"))
                .header("Host", base_url.trim_start_matches("http://"))
                .header("Upgrade", "websocket")
                .header("Connection", "upgrade")
                .header("Sec-WebSocket-Key", generate_key())
                .header("Sec-WebSocket-Version", "13")
                .body(())
                .unwrap(),
        ] {
            assert!(
                tokio_tungstenite::connect_async(request).await.is_err(),
                "WS must reject missing and URL-borne credentials"
            );
        }
    })
    .await;
}

/// 手工性能探针：release 模式下分别跑 push / pull，并输出服务端权威实际倍率。
/// 不设置机器相关的胜负阈值；结果用于同一台机器、同一提交上的相对比较。
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "手工性能探针：cargo test -p server --release --test ws publisher_modes_report_actual_speed -- --ignored --nocapture"]
async fn publisher_modes_report_actual_speed() {
    async fn measure(mode: &str) -> f64 {
        let mode = mode.to_owned();
        ServerFixture::run(1_000, |fixture| async move {
            let base_url = &fixture.base_url;
            let (id, handles) = fixture.session(100);
            let token = handles.session_token.clone();
            handles.set_speed(f64::INFINITY).await.unwrap();
            handles.set_running(true).await.unwrap();
            let ws_url = fixture.ws_url();
            let req = WsRequest::builder()
                .method("GET")
                .uri(format!("{ws_url}/ws?session_id={id}&delivery={mode}"))
                .header("authorization", format!("Bearer {token}"))
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
            metrics.actual_multiplier.expect("性能窗口应产生实际倍率")
        })
        .await
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

#[tokio::test]
async fn restored_session_forces_a_gated_resync_then_sends_a_fresh_baseline() {
    // Given: an authenticated pull-mode client with its initial baseline.
    ServerFixture::run(1_000, |fixture| async move {
        let base_url = &fixture.base_url;
        let mut setup: engine::SessionSetup =
            serde_json::from_value(sample_setup_json()).expect("sample setup must deserialize");
        // Settle Saturday into Sunday without scheduling next-opening NPC requests.
        setup.start_date = engine::CivilDate::from_iso("2030-01-05").unwrap();
        let (id, handles) = fixture.session_with_setup(setup.clone(), 45);
        let token = handles.session_token.clone();
        let before = handles
            .public_baseline()
            .await
            .expect("baseline must succeed");
        let mut completed_day = engine::session::protocol::ProtocolSession::new(setup.clone(), 45)
            .expect("fixture session must initialize");
        if completed_day.game().civil_clock().phase()
            == engine::session::CivilPhase::IntradayTrading
        {
            for _ in 0..setup.ticks_per_day {
                completed_day
                    .step_frame()
                    .expect("fixture market tick must succeed");
            }
        }
        completed_day
            .end_civil_day_update()
            .expect("fixture must complete its first civil day");
        let slot = completed_day
            .save()
            .expect("completed-day fixture must be saveable");
        let ws_url = fixture.ws_url();
        let request = WsRequest::builder()
            .method("GET")
            .uri(format!("{ws_url}/ws?session_id={id}&delivery=pull"))
            .header("authorization", format!("Bearer {token}"))
            .header("Host", base_url.trim_start_matches("http://"))
            .header("Upgrade", "websocket")
            .header("Connection", "upgrade")
            .header("Sec-WebSocket-Key", generate_key())
            .header("Sec-WebSocket-Version", "13")
            .body(())
            .unwrap();
        let (mut ws, _) = tokio_tungstenite::connect_async(request)
            .await
            .expect("authenticated WS handshake must succeed");
        let _initial = ws.next().await.expect("baseline frame").expect("WS frame");

        // When: a successful restore replaces the authoritative session.
        handles.restore(slot).await.expect("restore must succeed");
        let resync = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("timeline change must force resync")
            .expect("WS stream must stay open")
            .expect("resync frame must be readable")
            .into_text()
            .expect("resync frame must be text");
        let resync: serde_json::Value = serde_json::from_str(&resync).expect("resync must be JSON");
        assert_eq!(resync["ResyncRequired"]["reason"], "timeline_changed");

        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            serde_json::json!({ "SubmitIntent": { "request_id": 8,
                "intent": { "PlaceLimit": { "code": "600101", "side": "Buy",
                    "price": { "Fixed": 1000 }, "qty": 100 } } } })
            .to_string(),
        ))
        .await
        .expect("resync barrier 期间仍应允许写入队列");
        let queued = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .unwrap()
            .expect("WS 应保持连接")
            .expect("入队确认应可读")
            .into_text()
            .unwrap();
        let queued: serde_json::Value = serde_json::from_str(&queued).unwrap();
        assert_eq!(queued["CommandQueued"]["request_id"], 8);

        ws.send(tokio_tungstenite::tungstenite::Message::Text(
            r#"{"Resync":{}}"#.into(),
        ))
        .await
        .expect("resync command must be accepted");
        let baseline = tokio::time::timeout(Duration::from_secs(2), ws.next())
            .await
            .expect("resync must issue a baseline")
            .expect("WS stream must stay open")
            .expect("baseline frame must be readable")
            .into_text()
            .expect("baseline frame must be text");

        // Then: the replacement baseline establishes a fresh generation and revision before deltas.
        let baseline: serde_json::Value =
            serde_json::from_str(&baseline).expect("baseline must be JSON");
        assert_eq!(
            baseline["Baseline"]["timeline_generation"],
            before.timeline_generation + 1
        );
        assert_eq!(
            baseline["Baseline"]["public_revision"],
            before.public_revision + 1
        );
    })
    .await;
}

#[tokio::test]
async fn server_fixture_shutdown_stops_listener_and_removes_sessions() {
    let fixture = ServerFixture::start(1_000).await;
    let (id, handles) = fixture.session(101);
    let manager = fixture.manager.clone();
    let address = fixture.base_url.trim_start_matches("http://").to_owned();
    assert!(manager.lookup(&id).is_some());
    fixture.shutdown().await;
    assert!(manager.lookup(&id).is_none());
    assert_eq!(manager.active_session_count(), 0);
    assert!(matches!(
        handles.snapshot().await,
        Err(server::SendCommandError::ActorGone)
    ));
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}

#[tokio::test]
async fn server_fixture_cleans_resources_after_assertion_panic() {
    let captured = std::sync::Arc::new(std::sync::Mutex::new(None));
    let test_resources = captured.clone();
    let result = tokio::spawn(ServerFixture::run(1_000, |fixture| async move {
        let (id, handles) = fixture.session(102);
        let address = fixture.base_url.trim_start_matches("http://").to_owned();
        *test_resources.lock().unwrap() = Some((fixture.manager.clone(), id, handles, address));
        panic!("fixture 断言路径");
    }))
    .await;
    assert!(result.unwrap_err().is_panic());
    let (manager, id, handles, address) = captured
        .lock()
        .unwrap()
        .take()
        .expect("panic 前应保存资源句柄");
    assert!(manager.lookup(&id).is_none());
    assert!(matches!(
        handles.snapshot().await,
        Err(server::SendCommandError::ActorGone)
    ));
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}
