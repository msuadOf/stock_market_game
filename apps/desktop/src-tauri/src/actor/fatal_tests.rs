use super::failure::{failure_cause, failure_description};
use super::*;
use tauri::Listener;

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
    let app = tauri::test::mock_app();
    let (notices_tx, mut notices_rx) = mpsc::unbounded_channel();
    let (events_tx, mut events_rx) = mpsc::unbounded_channel();
    app.listen("engine-failure", move |event| {
        notices_tx.send(event.payload().to_owned()).unwrap();
    });
    app.listen(crate::ENGINE_EVENT_NAME, move |event| {
        events_tx.send(event.payload().to_owned()).unwrap();
    });
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
    let mut game = ProtocolSession::new(setup, 7).unwrap();
    let retained = game.step_frame().unwrap();
    game.enqueue_player_intent(
        engine::AccountId(0),
        engine::Intent::PlaceLimit {
            code: engine::StockCode("600101".into()),
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
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
    let mut actor = SessionActor {
        injected_step_failure: Some((successful_steps, fatal.clone())),
        speed_meter: SpeedMeter::new(tick),
        game,
        cmd_rx,
        tick_interval: Duration::from_millis(1),
        base_ms: 1,
        session_id: "fatal-session".to_owned(),
        app: app.handle().clone(),
        running: true,
        fastest,
        requested_speed: RequestedSpeed::Fixed { multiplier: 1.0 },
        pending_fixed_events: Vec::new(),
        last_fixed_publish: Instant::now(),
        timeline_id: "fatal-timeline".to_owned(),
        generation: 1,
        pause_preferences: PausePreferences::default(),
    };

    if fastest {
        actor.run_fastest_batch().await;
    } else {
        actor.tick_and_emit().await;
    }

    let payload = notices_rx.try_recv().expect("fatal notification required");
    let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
    assert_eq!(
        value,
        serde_json::json!({
            "session_id": "fatal-session", "timeline_id": "fatal-timeline",
            "code": "STEP_FATAL", "message": "引擎不变量校验失败（原始详情已脱敏）", "events": [],
            "where": "desktop.auto_step", "cause": null,
            "context": { "operation": "step", "tick": tick, "seq": seq, "day": actor.game.day(), "generation": "1" },
            "recoverable": false,
            "recoveryActions": ["停止当前会话；重新打开上一份有效日终存档或新局", "复制脱敏错误详情反馈"],
        })
    );
    assert!(!actor.running);
    assert!(cmd_tx.is_closed());
    actor
        .handle_command(SessionCommand::SetRunning { running: true })
        .await;
    assert!(!actor.running);
    assert_eq!(actor.game.tick(), tick);
    assert_eq!(actor.game.seq(), seq);
    assert_eq!(actor.game.business_state_hash().unwrap(), business);
    assert_eq!(
        serde_json::to_value(actor.game.game().save().unwrap()).unwrap(),
        saved_before
    );
    actor.tick_and_emit().await;
    actor.run_fastest_batch().await;
    assert_eq!(actor.game.tick(), tick);
    assert_eq!(actor.game.business_state_hash().unwrap(), business);
    assert!(notices_rx.try_recv().is_err());
    assert!(events_rx.try_recv().is_err());
    for _ in tick..30 {
        actor.game.step_frame().unwrap();
    }
    let civil = actor.game.end_civil_day_update().unwrap();
    assert_eq!(civil.refresh.intraday.len(), 30);
    assert_eq!(
        serde_json::to_value(&civil.refresh.intraday[0]).unwrap(),
        serde_json::to_value(retained).unwrap()
    );
    tokio::time::timeout(Duration::from_secs(1), actor.run())
        .await
        .unwrap();
    assert!(notices_rx.try_recv().is_err());
    assert!(events_rx.try_recv().is_err());
    println!("desktop fatal fastest={fastest} after={successful_steps} restored tick={tick} seq={seq} healthy=0: {payload}");
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
async fn fastest_third_step_failure_restores_the_entire_cycle() {
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
        "civil boundary is inconsistent".into(),
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
