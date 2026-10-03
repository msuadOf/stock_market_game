use super::*;
use engine::session::protocol::ReplayGuard;

async fn capture(fastest: bool, preferences: PausePreferences) {
    let mut setup = super::tests::diagnostic_setup();
    setup.start_date = engine::CivilDate::from_iso("2030-01-02").unwrap();
    let game = ProtocolSession::new(setup, 41).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        fastest,
        preferences,
        "protocol",
        "protocol-timeline",
    );
    harness.subscribe_engine_events();
    let receiver = harness
        .events_rx
        .as_mut()
        .expect("protocol 场景须订阅引擎事件");
    let actor = &mut harness.actor;
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
                    assert!(batch.runtime_snapshot.is_none());
                    let delta = batch.runtime_delta.as_ref().unwrap();
                    let last = batch.frames.last().unwrap();
                    assert_eq!(delta.tick, last.tick);
                    assert_eq!(delta.seq_from, batch.frames.first().unwrap().seq_from);
                    assert_eq!(delta.seq_to, last.seq_to);
                    assert!(delta
                        .accounts
                        .keys()
                        .all(|account| *account == engine::AccountId(0)));
                    if batch.frames.first().unwrap().tick == 1 {
                        assert!(delta.accounts.contains_key(&engine::AccountId(0)));
                        assert!(delta.working_orders.reset);
                    }
                    for frame in &batch.frames {
                        ticks.push(frame.tick);
                        assert!(!frame.timeseries_payload.markets.is_empty());
                    }
                }
                EngineUpdate::CivilUpdate(civil) => {
                    civil.validate().unwrap();
                    assert_eq!(ticks, (1..=30).collect::<Vec<_>>());
                    assert_eq!(civil.refresh.intraday.len(), 30);
                    assert_eq!(actor.pacing.is_running(), !preferences.pauses(civil));
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
