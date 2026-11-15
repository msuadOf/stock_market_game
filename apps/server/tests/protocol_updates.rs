#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use engine::session::protocol::{EngineUpdate, PausePreferences, ReplayGuard};
use engine::session::publication::Read;
use engine::session::OpaqueSubjectId;
use engine::{CivilDate, StockId};
use server::SessionManager;
use std::time::Duration;

async fn capture(fastest: bool, preferences: PausePreferences) {
    let manager = SessionManager::with_base_ms(1);
    let setup = fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap());
    let id = manager.new_session(setup, 41).unwrap();
    let handles = manager.lookup(&id).unwrap();
    let mut receiver = handles.subscribe_events();
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
                assert!(batch.runtime_snapshot.is_none());
                let delta = batch.runtime_delta.as_ref().unwrap();
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
                    assert_eq!(frame.facts.len(), frame.events().len());
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

#[tokio::test]
async fn stalled_observer_does_not_hold_over_live_intraday_price_lines() {
    let database = native_store::NativeDatabase::open_in_memory().unwrap();
    let guest = server::identity::IdentityService::new(database.clone())
        .guest()
        .unwrap();
    let manager = SessionManager::with_database(database);
    let first_day = CivilDate::from_iso("2030-01-02").unwrap();
    let subject = OpaqueSubjectId::new(guest.subject.subject_id).unwrap();
    let (id, created) = manager
        .new_shared_session(fixture::civil_setup(first_day), 41, subject.clone())
        .unwrap();
    assert!(created);
    let handles = manager.lookup(&id).unwrap();
    let mut stalled = handles.observe_for(subject).await.unwrap();
    handles
        .set_pause_preferences(
            1,
            PausePreferences {
                pause_after_close: false,
                pause_before_open: true,
            },
        )
        .await
        .unwrap();
    handles.set_speed(f64::INFINITY).await.unwrap();
    handles.set_running(true).await.unwrap();

    // 不消费观察队列、不确认消息；只为检验日界，通过权威控制查询等待停在次日开盘前。
    tokio::time::timeout(Duration::from_secs(3), async {
        while handles.speed_metrics().await.unwrap().running {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("观察者停滞不应阻挡权威推进跨日");

    let saved = handles.save(1, None).await.unwrap();
    let stock = StockId(0);
    assert_eq!(saved.civil_clock.settled_through, Some(first_day));
    assert_eq!(saved.civil_clock.current_date, first_day.next().unwrap());
    assert_eq!(saved.snapshot.tick, fixture::TICKS_PER_DAY);
    assert!(saved.market_minute_closes.values().all(Vec::is_empty));
    assert!(saved.market_minute_bars.values().all(Vec::is_empty));
    let finished = saved.snapshot.daily_candles[&stock].last().unwrap();
    let epoch = CivilDate::from_iso("1970-01-01").unwrap();
    assert_eq!(finished.time, first_day.days_since(epoch) * 86_400);
    // 停滞端现在才读取第一包，证明确有旧日分时线，而不是空行情恰好留下空桶。
    let Read::Item { ordinal: 1, value } = stalled.observer.poll() else {
        panic!("观察者应收到日结以前的行情前缀");
    };
    let Some(EngineUpdate::TickBatch(batch)) =
        value.into_projected(&mut stalled.initial_orders).update
    else {
        panic!("观察者第一包应为交易 tick 批次");
    };
    assert!(batch.frames.iter().any(|frame| {
        frame
            .timeseries_payload
            .continuous_points
            .contains_key(&stock)
    }));
    manager.remove(&id).unwrap().shutdown().await.unwrap();
}
