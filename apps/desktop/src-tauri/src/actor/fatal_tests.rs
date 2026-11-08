use super::failure::{failure_cause, failure_description};
use super::*;

#[test]
fn producer_failure_contains_real_location_and_recovery_details() {
    let error = engine::session::StepFatal::InvariantViolation {
        location: "ReceiptAggregation::validate".into(),
        description: "receipt chain broke".into(),
    };
    let value = serde_json::to_value(super::failure::HostFailure::step(error)).unwrap();
    assert_eq!(value["where"], "ReceiptAggregation::validate");
    assert_eq!(value["context"]["operation"], "step");
    assert_eq!(value["recoverable"], false);
    assert!(value["recoveryActions"]
        .as_array()
        .is_some_and(|actions| !actions.is_empty()));
    assert!(value["cause"].is_null());
}

async fn assert_auto_step_fatal(fastest: bool, successful_steps: usize) {
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
    let mut game = ProtocolSession::new(setup, 7).unwrap();
    let retained = game.step_frame().unwrap();
    game.enqueue_player_intent(
        engine::AccountId(0),
        engine::Intent::PlaceLimit {
            code: engine::StockId(0),
            side: engine::Side::Buy,
            price: engine::LimitPrice::Fixed(engine::Money::from_cents(1000)),
            qty: 100,
        },
    )
    .unwrap();
    let tick = game.tick();
    let seq = game.seq();
    let business = game.business_state_hash().unwrap();
    let saved_before = serde_json::to_value(game.game().save().unwrap()).unwrap();
    let fatal = engine::session::StepFatal::InvariantViolation {
        description: "desktop injected failure".to_owned(),
        location: "desktop.auto_step".to_owned(),
    };
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        fastest,
        PausePreferences::default(),
        "fatal-session",
        "fatal-timeline",
    );
    harness.subscribe_failures();
    harness.subscribe_engine_events();
    let notices_rx = harness
        .failures_rx
        .as_mut()
        .expect("fatal 场景须订阅失败事件");
    let events_rx = harness
        .events_rx
        .as_mut()
        .expect("fatal 场景须订阅引擎事件");
    let actor = &mut harness.actor;
    let capture = Arc::new(std::sync::Mutex::new(StepFailureCapture::default()));
    actor.injected_step_failure = Some((successful_steps, fatal.clone(), Arc::clone(&capture)));

    if fastest {
        actor.run_fastest_batch().await;
        actor.drain_consumed();
    } else {
        actor.tick_and_emit().await;
        actor.drain_consumed();
    }

    let committed_tick = actor.game.tick();
    let committed_seq = actor.game.seq();
    assert_eq!(committed_tick, tick + successful_steps as u64);
    let committed_business = actor.game.business_state_hash().unwrap();
    let committed_save = serde_json::to_value(actor.game.game().save().unwrap()).unwrap();
    if successful_steps == 0 {
        assert_eq!(committed_business, business);
        assert_eq!(committed_save, saved_before);
    }
    let mut replay = engine::session::protocol::ReplayGuard::new(tick, seq);
    let mut delivered = Vec::new();
    let mut delivered_frames = Vec::new();
    while let Ok(payload) = events_rx.try_recv() {
        let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
        let update: EngineUpdate = serde_json::from_value(value["update"].clone()).unwrap();
        let EngineUpdate::TickBatch(batch) = &update else {
            panic!("失败前缀只能包含真实已成功市场帧")
        };
        delivered.extend(batch.frames.iter().map(|frame| frame.tick));
        delivered_frames.extend(batch.frames.iter().cloned());
        replay.ingest(&update).unwrap();
    }
    assert_eq!(
        delivered,
        (1..=successful_steps as u64)
            .map(|offset| tick + offset)
            .collect::<Vec<_>>()
    );
    let (expected_saved, expected_origin, expected_business, expected_frames) = {
        let capture = capture.lock().unwrap();
        (
            capture.saved.clone().unwrap(),
            capture.origin.unwrap(),
            capture.business.unwrap(),
            capture.frames.clone(),
        )
    };
    assert_eq!(
        committed_save, expected_saved,
        "失败处理不能改写同 Session 失败前已提交的全部事实"
    );
    assert_eq!(actor.game.runtime_mirror_origin(), expected_origin);
    assert_eq!(
        actor.game.business_state_hash().unwrap(),
        expected_business,
        "真实业务指纹必须与同 Session fatal 返回前完全一致"
    );
    assert_eq!(
        serde_json::to_value(&delivered_frames).unwrap(),
        serde_json::to_value(&expected_frames).unwrap(),
        "全部成功前缀事实必须原样交付且恰好一次"
    );
    let payload = notices_rx.try_recv().expect("fatal notification required");
    let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "session_id": "fatal-session", "timeline_id": "fatal-timeline",
            "code": "STEP_FATAL", "message": "引擎不变量校验失败（原始详情已脱敏）", "events": [],
            "where": "desktop.auto_step", "cause": null,
            "context": { "operation": "step", "tick": committed_tick, "seq": committed_seq, "day": actor.game.day(), "generation": "1" },
            "recoverable": false,
            "recoveryActions": ["停止当前会话；重新打开上一份有效日终存档或新局", "复制脱敏错误详情反馈"],
        })
    );
    assert!(!actor.pacing.is_running());
    assert!(harness.cmd_tx.is_closed());
    assert!(actor
        .game
        .shared_ingress()
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: engine::StockId(0),
                id: engine::OrderId(1)
            }
        )
        .is_err());
    let (reply, rejected) = oneshot::channel();
    actor
        .handle_command(SessionCommand::SetRunning {
            running: true,
            reply,
        })
        .await;
    assert!(rejected.await.is_err());
    assert!(!actor.pacing.is_running());
    assert_eq!(actor.game.tick(), committed_tick);
    assert_eq!(actor.game.seq(), committed_seq);
    assert_eq!(
        actor.game.business_state_hash().unwrap(),
        committed_business
    );
    assert_eq!(
        serde_json::to_value(actor.game.game().save().unwrap()).unwrap(),
        committed_save
    );
    actor.tick_and_emit().await;
    actor.drain_consumed();
    actor.run_fastest_batch().await;
    actor.drain_consumed();
    assert_eq!(actor.game.tick(), committed_tick);
    assert_eq!(
        actor.game.business_state_hash().unwrap(),
        committed_business
    );
    assert!(notices_rx.try_recv().is_err());
    assert!(events_rx.try_recv().is_err());
    for _ in committed_tick..30 {
        actor.game.step_frame().unwrap();
    }
    let civil = actor.game.end_civil_day_update().unwrap();
    assert_eq!(civil.refresh.intraday.len(), 30);
    let expected_prefix = std::iter::once(retained)
        .chain(expected_frames.iter().cloned())
        .collect::<Vec<_>>();
    assert_eq!(
        serde_json::to_value(&civil.refresh.intraday[..expected_prefix.len()]).unwrap(),
        serde_json::to_value(expected_prefix).unwrap(),
        "日终 retained 必须保留失败前全部真实帧"
    );
    tokio::time::timeout(Duration::from_secs(1), harness.actor.run())
        .await
        .unwrap();
    assert!(notices_rx.try_recv().is_err());
    assert!(events_rx.try_recv().is_err());
    println!("desktop fatal fastest={fastest} after={successful_steps} committed tick={committed_tick} seq={committed_seq} host_stopped=1: {payload}");
}

#[tokio::test]
async fn fixed_high_speed_production_batches_every_due_frame_before_ipc() {
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
    setup.ticks_per_day = 120;
    setup.npcs.inst_count = 0;
    let mut game = ProtocolSession::new(setup, 7).unwrap();
    game.step_frame().unwrap();
    game.prepare_public_baseline();
    let initial_tick = game.tick();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "batch",
        "timeline",
    );
    harness.subscribe_engine_events();
    harness.actor.pacing.tick_interval = Duration::from_millis(1);
    harness.actor.last_fixed_publish -= Duration::from_millis(16);
    let before_publish = harness.actor.last_fixed_publish;
    harness.actor.tick_and_emit().await;
    harness.actor.drain_consumed();
    let payload = harness.events_rx.as_mut().unwrap().try_recv().unwrap();
    let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
    let frames = value["update"]["TickBatch"]["frames"].as_array().unwrap();
    assert!(frames.len() > 1, "固定高倍率必须在 Rust 聚合后跨 IPC");
    for pair in frames.windows(2) {
        assert_eq!(pair[0]["seq_to"], pair[1]["seq_from"]);
        assert_eq!(
            pair[0]["tick"].as_u64().unwrap() + 1,
            pair[1]["tick"].as_u64().unwrap()
        );
    }
    assert!(harness.events_rx.as_mut().unwrap().try_recv().is_err());
    let first_tick = harness.actor.game.tick();
    assert_eq!(
        harness.actor.last_fixed_publish,
        before_publish + Duration::from_millis(first_tick - initial_tick)
    );
    harness.actor.last_fixed_publish -= Duration::from_millis(16);
    let second_before_publish = harness.actor.last_fixed_publish;
    harness.actor.tick_and_emit().await;
    harness.actor.drain_consumed();
    let second_payload = harness.events_rx.as_mut().unwrap().try_recv().unwrap();
    let second_value: serde_json::Value = serde_json::from_str(&second_payload).unwrap();
    let second_frames = second_value["update"]["TickBatch"]["frames"]
        .as_array()
        .unwrap();
    assert!(second_frames.len() > 1);
    assert_eq!(second_frames[0]["tick"].as_u64().unwrap(), first_tick + 1);
    assert_eq!(
        second_frames[0]["seq_from"],
        frames.last().unwrap()["seq_to"]
    );
    assert_eq!(
        harness.actor.last_fixed_publish,
        second_before_publish + Duration::from_millis(harness.actor.game.tick() - first_tick)
    );
}

#[tokio::test]
async fn desktop_controls_resolve_only_after_actor_applies_them() {
    let game = ProtocolSession::new(super::tests::diagnostic_setup(), 7).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "control",
        "timeline",
    );
    let handles = SessionHandles {
        ingress: Arc::clone(&harness.actor.ingress),
        cmd_tx: harness.cmd_tx.clone(),
    };
    let command = handles.set_running(false);
    tokio::pin!(command);
    assert!(tokio::time::timeout(Duration::from_millis(1), &mut command)
        .await
        .is_err());
    assert!(harness.actor.pacing.is_running());
    let queued = harness.actor.cmd_rx.recv().await.unwrap();
    harness.actor.handle_command(queued).await;
    command.await.unwrap();
    assert!(!harness.actor.pacing.is_running());
}

#[tokio::test]
async fn fixed_cold_batch_preserves_unfinished_tick_debt() {
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
    setup.ticks_per_day = 120;
    setup.npcs.inst_count = 0;
    let game = ProtocolSession::new(setup, 17).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "cold",
        "timeline",
    );
    harness.subscribe_engine_events();
    harness.actor.pacing.tick_interval = Duration::from_millis(1);
    harness.actor.last_fixed_publish -= Duration::from_millis(16);
    let original = harness.actor.last_fixed_publish;
    harness.actor.tick_and_emit().await;
    harness.actor.drain_consumed();
    let completed = harness.actor.game.tick();
    assert!(completed > 0);
    assert_eq!(
        harness.actor.last_fixed_publish,
        original + Duration::from_millis(completed)
    );
    assert!(
        harness.actor.last_fixed_publish.elapsed()
            >= Duration::from_millis(16).saturating_sub(Duration::from_millis(completed))
    );
    let payload = harness.events_rx.as_mut().unwrap().try_recv().unwrap();
    let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(
        value["update"]["TickBatch"]["frames"]
            .as_array()
            .unwrap()
            .len() as u64,
        completed
    );
}

#[test]
fn disconnected_consumer_delivers_fixed_prefix_before_fatal() {
    use tauri::Listener;
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
    setup.npcs.inst_count = 0;
    let game = ProtocolSession::new(setup, 7).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "disconnected",
        "one",
    );
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();
    for name in [crate::ENGINE_EVENT_NAME, "engine-failure"] {
        let sender = events_tx.clone();
        harness.app.listen(name, move |event| {
            sender.send((name, event.payload().to_owned())).unwrap();
        });
    }
    let first = harness
        .actor
        .game
        .advance_core_tick()
        .unwrap()
        .consume()
        .unwrap();
    let (input, receiver) = std::sync::mpsc::channel();
    drop(receiver);
    let (sender, output) = std::sync::mpsc::channel();
    assert!(sender.send(Ok(first)).is_ok());
    drop(sender);
    harness.actor.consumer = Some(TickConsumer {
        input: Some(input),
        output,
        worker: None,
    });
    harness.actor.pending_ticks = 1;
    harness.actor.fixed_batches.push_back((1, false));
    harness.actor.run_cycle_inner(1, true);
    harness.actor.finish_fixed_batch();
    let (first_name, first_payload) = events_rx.try_recv().unwrap();
    let (second_name, failure_payload) = events_rx.try_recv().unwrap();
    assert_eq!(first_name, crate::ENGINE_EVENT_NAME);
    let update: serde_json::Value = serde_json::from_str(&first_payload).unwrap();
    assert_eq!(update["update"]["TickBatch"]["frames"][0]["tick"], 1);
    assert_eq!(second_name, "engine-failure");
    let failure: serde_json::Value = serde_json::from_str(&failure_payload).unwrap();
    assert!(failure["message"].as_str().unwrap().contains("已提交"));
    assert_eq!(harness.actor.game.tick(), 2);
    assert!(events_rx.try_recv().is_err());
}

#[tokio::test]
async fn fixed_auto_step_emits_one_fatal_and_stops() {
    assert_auto_step_fatal(false, 0).await;
}

#[tokio::test]
async fn fastest_auto_step_emits_one_fatal_and_stops() {
    assert_auto_step_fatal(true, 0).await;
}

#[tokio::test]
async fn fastest_third_step_failure_preserves_committed_prefix_and_stops() {
    assert_auto_step_fatal(true, 2).await;
}

#[test]
fn step_fatal_maps_to_the_stable_host_code() {
    let fatal = engine::session::StepFatal::InvariantViolation {
        location: "desktop.step".into(),
        description: "receipt chain broke".into(),
    };
    let expected_message = "引擎不变量校验失败（原始详情已脱敏）";
    let failure = super::failure::HostFailure::from(fatal);
    assert_eq!(failure.code, "STEP_FATAL");
    assert_eq!(failure.message, expected_message);
    let value = serde_json::to_value(&failure).unwrap();
    assert_eq!(value["code"], "STEP_FATAL");
    assert_eq!(value["message"], expected_message);
    assert_eq!(value["where"], "desktop.step");
    assert_eq!(value["cause"], serde_json::Value::Null);
    assert_eq!(value["context"], serde_json::json!({ "operation": "step" }));
    assert_eq!(value["recoverable"], false);
    assert_eq!(
        value["recoveryActions"],
        serde_json::json!([
            "停止当前会话；重新打开上一份有效日终存档或新局",
            "复制脱敏错误详情反馈"
        ])
    );
}

#[test]
fn civil_settlement_error_maps_to_the_cross_host_failure_code() {
    let failure = super::failure::HostFailure::civil(engine::SessionError::InvalidSave(
        engine::error::ValidationFault::bare(3012),
    ));
    assert_eq!(failure.code, "CIVIL_DAY_SETTLEMENT_FAILED");
    assert_eq!(
        failure.message,
        "日终存档或协议状态校验失败（原始详情已脱敏）"
    );
}
#[test]
fn producer_failure_preserves_real_source_chain_without_private_state() {
    let error =
        engine::SessionError::Closing(engine::accounting::closing::ClosingError::Accounting(
            engine::accounting::AccountingError::AmountOverflow {
                op: "add",
                detail: "token=private-secret account cash=987654321".into(),
            },
        ));
    let value = serde_json::to_value(super::failure::HostFailure::civil(error)).unwrap();
    assert_eq!(value["code"], "CIVIL_DAY_SETTLEMENT_FAILED");
    assert!(value["cause"].is_object());
    assert!(value["cause"]["cause"].is_object());
    assert!(value["cause"]["cause"]["cause"].is_null());
    let encoded = value.to_string();
    assert!(!encoded.contains("private-secret"));
    assert!(!encoded.contains("987654321"));
    assert!(!encoded.contains("recovery_actions"));
}
#[test]
fn producer_failure_redacts_real_bare_amount_overflow_operands() {
    let private_operand = 2_718_281_828_459_045_235_i128;
    let unknown = std::io::Error::other(private_operand.to_string());
    assert_eq!(
        failure_description(&unknown),
        ("ERROR_DETAILS_REDACTED", "原始错误类型未识别，详情未公开")
    );
    assert!(failure_cause(&unknown).is_none());
    let overflow = engine::accounting::AccountingAmount::MAX
        .add(engine::accounting::AccountingAmount::from_cents(
            private_operand,
        ))
        .unwrap_err();
    let engine::accounting::AccountingError::AmountOverflow { ref detail, .. } = overflow else {
        panic!("fixture must produce a real amount overflow")
    };
    assert_eq!(detail, &format!("{} + {private_operand}", i128::MAX));
    let error = engine::SessionError::Closing(
        engine::accounting::closing::ClosingError::Accounting(overflow),
    );
    let value = serde_json::to_value(super::failure::HostFailure::civil(error)).unwrap();
    println!("A08_BARE_AMOUNT_FAILURE={value}");
    let encoded = value.to_string();
    assert!(
        !encoded.contains(&private_operand.to_string()),
        "bare private operand leaked"
    );
    assert!(
        !encoded.contains(&i128::MAX.to_string()),
        "bare private operand leaked"
    );
    assert_eq!(value["cause"]["code"], "CLOSING_ACCOUNTING_FAILED");
    assert_eq!(
        value["cause"]["cause"]["code"],
        "ACCOUNTING_AMOUNT_OVERFLOW"
    );
    assert_eq!(
        value["cause"]["cause"]["message"],
        "公司会计金额运算溢出（操作数已脱敏）"
    );
    assert!(value["cause"]["cause"]["cause"].is_null());
}
