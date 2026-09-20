use super::*;
use tauri::Listener;

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
            price: engine::Money::from_cents(1000),
            qty: 100,
        },
    )
    .unwrap();
    let tick = game.tick();
    let seq = game.seq();
    let business = game.business_state_hash().unwrap();
    let saved_before = serde_json::to_value(game.save().unwrap()).unwrap();
    let fatal = engine::session::StepFatal::InvariantViolation {
        description: "desktop injected failure".to_owned(),
        location: "desktop.auto_step".to_owned(),
    };
    let (cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
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
            "code": "STEP_FATAL", "message": fatal.to_string(), "events": [],
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
        serde_json::to_value(actor.game.save().unwrap()).unwrap(),
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
fn both_step_fatal_variants_map_to_the_same_stable_host_code() {
    let setup = super::tests::diagnostic_setup();
    let expected = ProtocolSession::new(setup.clone(), 1)
        .unwrap()
        .business_state_hash()
        .unwrap();
    let observed = ProtocolSession::new(setup, 2)
        .unwrap()
        .business_state_hash()
        .unwrap();
    let variants = [
        engine::session::StepFatal::InvariantViolation {
            location: "desktop.step".into(),
            description: "receipt chain broke".into(),
        },
        engine::session::StepFatal::Internal { expected, observed },
    ];

    for fatal in variants {
        let expected_message = fatal.to_string();
        let failure = super::failure::HostFailure::from(fatal);
        assert_eq!(failure.code, "STEP_FATAL");
        assert_eq!(failure.message, expected_message);
        assert_eq!(
            serde_json::to_value(&failure).unwrap(),
            serde_json::json!({ "code": "STEP_FATAL", "message": expected_message })
        );
    }
}
