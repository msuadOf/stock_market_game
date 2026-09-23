use super::*;
use engine::session::protocol::ReplayGuard;
use tauri::Listener;

async fn capture(fastest: bool, preferences: PausePreferences) {
    let app = tauri::test::mock_app();
    let (sender, mut receiver) = mpsc::unbounded_channel();
    app.listen(crate::ENGINE_EVENT_NAME, move |event| {
        sender.send(event.payload().to_owned()).unwrap();
    });
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
    let game = ProtocolSession::new(setup, 41).unwrap();
    let (_cmd_tx, cmd_rx) = mpsc::channel(COMMAND_CHANNEL_CAPACITY);
    let mut actor = SessionActor {
        injected_step_failure: None,
        speed_meter: SpeedMeter::new(0),
        game,
        cmd_rx,
        tick_interval: Duration::from_millis(1),
        base_ms: 1,
        session_id: "protocol".into(),
        app: app.handle().clone(),
        running: true,
        fastest,
        requested_speed: RequestedSpeed::Fixed { multiplier: 1.0 },
        pending_fixed_events: Vec::new(),
        last_fixed_publish: Instant::now(),
        timeline_id: "protocol-timeline".into(),
        generation: 1,
        pause_preferences: preferences,
    };
    let mut guard = ReplayGuard::new(0, 0);
    let mut ticks = Vec::new();
    loop {
        if fastest {
            actor.run_fastest_batch().await;
        } else {
            actor.tick_and_emit().await;
        }
        while let Ok(payload) = receiver.try_recv() {
            let value: serde_json::Value = serde_json::from_str(&payload).unwrap();
            assert!(value.get("events").is_none());
            let update: EngineUpdate = serde_json::from_value(value["update"].clone()).unwrap();
            guard.ingest(&update).unwrap();
            match &update {
                EngineUpdate::TickBatch(batch) => {
                    batch.validate().unwrap();
                    if !fastest {
                        assert_eq!(batch.frames.len(), 1);
                    }
                    assert_eq!(
                        batch.runtime_snapshot.as_ref().unwrap().tick,
                        batch.frames.last().unwrap().tick
                    );
                    for frame in &batch.frames {
                        ticks.push(frame.tick);
                        assert!(!frame.timeseries_payload.markets.is_empty());
                    }
                }
                EngineUpdate::CivilUpdate(civil) => {
                    civil.validate().unwrap();
                    assert_eq!(ticks, (1..=30).collect::<Vec<_>>());
                    assert_eq!(civil.refresh.intraday.len(), 30);
                    assert_eq!(actor.running, !preferences.pauses(civil));
                    println!("desktop fastest={fastest}: {payload}");
                    actor
                        .handle_command(SessionCommand::SetRunning { running: true })
                        .await;
                    actor.tick_and_emit().await;
                    let next: serde_json::Value =
                        serde_json::from_str(&receiver.try_recv().unwrap()).unwrap();
                    let next: EngineUpdate =
                        serde_json::from_value(next["update"].clone()).unwrap();
                    assert!(matches!(next, EngineUpdate::TickBatch(_)));
                    guard.ingest(&next).unwrap();
                    assert!(receiver.try_recv().is_err());
                    return;
                }
            }
        }
    }
}

#[tokio::test]
async fn fixed_complete_frames_auto_continue() {
    capture(false, PausePreferences::default()).await;
}

#[tokio::test]
async fn fastest_complete_frames_auto_continue() {
    capture(true, PausePreferences::default()).await;
}

#[tokio::test]
async fn after_close_publishes_before_pausing_and_resumes_once() {
    capture(
        false,
        PausePreferences {
            pause_after_close: true,
            pause_before_open: false,
        },
    )
    .await;
}

#[tokio::test]
async fn before_open_publishes_before_pausing_and_resumes_once() {
    capture(
        true,
        PausePreferences {
            pause_after_close: false,
            pause_before_open: true,
        },
    )
    .await;
}
