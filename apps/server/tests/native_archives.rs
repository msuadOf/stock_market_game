#![cfg(feature = "host-parity")]

#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use engine::session::protocol::SaveCandidateKey;
use engine::CivilDate;
use native_store::NativeDatabase;
use server::SessionManager;

#[tokio::test]
async fn server_writes_closed_day_without_any_client_save_request_and_restarts() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let manager = SessionManager::with_database(database.clone());
    let id = manager
        .new_session(
            fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()),
            41,
        )
        .unwrap();
    let handles = manager.lookup(&id).unwrap();
    assert!(database.load("current").unwrap().is_none());
    let update = handles.advance_civil_day(1).await.unwrap();
    let saved = database.load("current").unwrap().unwrap();
    assert_eq!(
        saved.civil_clock.settled_through,
        Some(update.boundary.settled_date)
    );
    assert_eq!(
        serde_json::to_value(&saved).unwrap(),
        serde_json::to_value(
            handles
                .save(
                    1,
                    Some(SaveCandidateKey {
                        seq: update.seq_to,
                        settled_date: update.boundary.settled_date
                    })
                )
                .await
                .unwrap()
        )
        .unwrap()
    );
    let next_manager = SessionManager::with_database(database);
    let resumed = next_manager.resume_session("current").unwrap().unwrap();
    let snapshot = next_manager
        .lookup(&resumed)
        .unwrap()
        .snapshot()
        .await
        .unwrap();
    assert_eq!(snapshot.tick, saved.snapshot.tick);
}

#[tokio::test]
async fn server_restore_renews_archive_writer_before_replaying_day_end() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let manager = SessionManager::with_database(database.clone());
    let id = manager
        .new_session(
            fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()),
            41,
        )
        .unwrap();
    let handles = manager.lookup(&id).unwrap();
    let saturday = handles.advance_civil_day(1).await.unwrap();
    let checkpoint = database.load("current").unwrap().unwrap();
    assert_eq!(checkpoint.snapshot.seq, saturday.seq_to);
    let sunday = handles.advance_civil_day(1).await.unwrap();
    assert!(sunday.seq_to > saturday.seq_to);
    assert_eq!(
        database.load("current").unwrap().unwrap().snapshot.seq,
        sunday.seq_to,
    );

    handles.restore(checkpoint).await.unwrap();
    let mut events = handles.subscribe_events();
    let replayed = handles.advance_civil_day(2).await.unwrap();
    assert_eq!(replayed.seq_to, sunday.seq_to);
    assert_eq!(replayed.boundary.settled_date, sunday.boundary.settled_date);
    let mut published = false;
    loop {
        match events.try_recv() {
            Ok(update) => {
                assert!(
                    update.failure.is_none(),
                    "恢复后的正常日终不得被旧时间线的 SQLite 写入游标拒绝：{:?}",
                    update.failure
                );
                if let Some(engine::session::protocol::ProtocolUpdate::CivilUpdate(day)) =
                    update.update
                {
                    assert_eq!(update.timeline_generation, 2);
                    assert_eq!(day.seq_to, replayed.seq_to);
                    published = true;
                }
            }
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(error) => panic!("恢复后日终广播读取失败：{error}"),
        }
    }
    assert!(published, "应发布新世代重放完成的日终");
    let persisted = database.load("current").unwrap().unwrap();
    assert_eq!(
        persisted.civil_clock.settled_through,
        Some(replayed.boundary.settled_date)
    );
    assert_eq!(persisted.snapshot.seq, replayed.seq_to);
}

#[tokio::test]
async fn server_restore_without_archive_ownership_keeps_active_timeline_and_ingress() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let manager = SessionManager::with_database(database.clone());
    let id = manager
        .new_session(
            fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()),
            41,
        )
        .unwrap();
    let handles = manager.lookup(&id).unwrap();
    handles.advance_civil_day(1).await.unwrap();
    let checkpoint = database.load("current").unwrap().unwrap();
    let sunday = handles.advance_civil_day(1).await.unwrap();
    let _new_owner = native_store::ArchiveWriter::activate(database.clone(), "current").unwrap();
    let rejected = handles.restore(checkpoint).await.unwrap_err();
    assert!(rejected.to_string().contains("未能接管日终存档写入"));
    assert_eq!(handles.snapshot().await.unwrap().seq, sunday.seq_to);
    // 恢复被拒后，原世代仍接单，不能留下已关闭或换代但未安装的 ingress。
    handles
        .enqueue(
            1,
            engine::Intent::PlaceLimit {
                code: engine::StockId(0),
                side: engine::Side::Buy,
                price: engine::LimitPrice::Fixed(engine::Money::from_cents(1000)),
                qty: 100,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        database.load("current").unwrap().unwrap().snapshot.seq,
        sunday.seq_to
    );
}

#[tokio::test]
async fn server_failed_write_preserves_previous_archive_and_broadcasts_failure() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let mut game = engine::session::protocol::ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()),
        41,
    )
    .unwrap();
    let update = game.end_civil_day_update().unwrap();
    let previous = native_store::DayEndCandidate::capture(
        &game,
        &SaveCandidateKey {
            seq: update.seq_to,
            settled_date: update.boundary.settled_date,
        },
    )
    .unwrap();
    native_store::ArchiveWriter::activate(database.clone(), "current")
        .unwrap()
        .save_day_end(&previous)
        .unwrap();
    let manager = SessionManager::with_database(database.clone());
    let id = manager.resume_session("current").unwrap().unwrap();
    let handles = manager.lookup(&id).unwrap();
    let mut events = handles.subscribe_events();
    database
        .with_connection(|connection| {
            connection.execute_batch("PRAGMA query_only = ON")?;
            Ok(())
        })
        .unwrap();
    handles.advance_civil_day(1).await.unwrap();
    let mut updates = Vec::new();
    loop {
        match events.try_recv() {
            Ok(update) => updates.push(update),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(error) => panic!("日终广播读取失败：{error}"),
        }
    }
    let failure = updates
        .iter()
        .find_map(|update| update.failure.as_ref())
        .expect("SQLite 写入失败必须广播");
    assert_eq!(failure.code, "DAY_END_ARCHIVE_FAILED");
    assert!(failure.recoverable);
    assert_eq!(
        serde_json::to_value(database.load("current").unwrap().unwrap()).unwrap(),
        serde_json::to_value(previous).unwrap()
    );
}
