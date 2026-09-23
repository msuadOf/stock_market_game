#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use engine::session::protocol::{EngineUpdate, PausePreferences, ReplayGuard};
use engine::CivilDate;
use server::SessionManager;
use std::time::Duration;

async fn capture(fastest: bool, preferences: PausePreferences) {
    let manager = SessionManager::with_base_ms(1);
    let setup = fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap());
    let id = manager.new_session(setup, 41).unwrap();
    let handles = manager.lookup(&id).unwrap();
    let mut receiver = handles.event_tx.subscribe();
    handles.set_pause_preferences(1, preferences).await.unwrap();
    if fastest {
        handles.set_speed(f64::INFINITY).await.unwrap();
    }
    handles.set_running(true).await.unwrap();
    let mut guard = ReplayGuard::new(0, 0);
    let mut ticks = Vec::new();
    loop {
        let envelope = tokio::time::timeout(Duration::from_secs(3), receiver.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(envelope.failure.is_none(), "{:?}", envelope.failure);
        let update = envelope.update.unwrap();
        guard.ingest(&update).unwrap();
        match &update {
            EngineUpdate::TickBatch(batch) => {
                batch.validate().unwrap();
                if !fastest {
                    assert_eq!(batch.frames.len(), 1);
                }
                let last = batch.frames.last().unwrap();
                assert_eq!(batch.runtime_snapshot.as_ref().unwrap().tick, last.tick);
                for frame in &batch.frames {
                    ticks.push(frame.tick);
                    assert_eq!(frame.facts.len(), frame.events.len());
                    assert!(!frame.timeseries_payload.markets.is_empty());
                }
            }
            EngineUpdate::CivilUpdate(civil) => {
                civil.validate().unwrap();
                assert_eq!(ticks, (1..=fixture::TICKS_PER_DAY).collect::<Vec<_>>());
                assert_eq!(civil.refresh.intraday.len(), ticks.len());
                println!(
                    "server fastest={fastest}: {}",
                    serde_json::to_string(&update).unwrap()
                );
                let paused = preferences.pauses(civil);
                assert_eq!(handles.speed_metrics().await.unwrap().running, !paused);
                if paused {
                    assert!(receiver.try_recv().is_err());
                    handles.set_running(true).await.unwrap();
                    let next = tokio::time::timeout(Duration::from_secs(3), receiver.recv())
                        .await
                        .unwrap()
                        .unwrap();
                    let next = next.update.unwrap();
                    assert!(matches!(next, EngineUpdate::TickBatch(_)));
                    guard.ingest(&next).unwrap();
                }
                break;
            }
        }
    }
    manager.remove(&id).unwrap().shutdown().await.unwrap();
}

#[tokio::test]
async fn fixed_frames_auto_continue_by_default() {
    capture(false, PausePreferences::default()).await;
}

#[tokio::test]
async fn fastest_preserves_consecutive_frames_and_civil_barrier() {
    capture(true, PausePreferences::default()).await;
}

#[tokio::test]
async fn after_close_pauses_after_barrier_and_resumes_once() {
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
async fn before_open_pauses_after_barrier_and_resumes_once() {
    capture(
        true,
        PausePreferences {
            pause_after_close: false,
            pause_before_open: true,
        },
    )
    .await;
}
