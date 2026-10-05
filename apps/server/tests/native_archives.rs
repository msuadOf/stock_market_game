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
    let id = manager.new_session(fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()), 41).unwrap();
    let handles = manager.lookup(&id).unwrap();
    assert!(database.load("current").unwrap().is_none());
    let update = handles.advance_civil_day(1).await.unwrap();
    let saved = database.load("current").unwrap().unwrap();
    assert_eq!(saved.civil_clock.settled_through, Some(update.boundary.settled_date));
    assert_eq!(serde_json::to_value(&saved).unwrap(), serde_json::to_value(handles.save(1, Some(SaveCandidateKey { seq: update.seq_to, settled_date: update.boundary.settled_date })).await.unwrap()).unwrap());
    let next_manager = SessionManager::with_database(database);
    let resumed = next_manager.resume_session("current").unwrap().unwrap();
    let snapshot = next_manager.lookup(&resumed).unwrap().snapshot().await.unwrap();
    assert_eq!(snapshot.tick, saved.snapshot.tick);
}

#[tokio::test]
async fn server_failed_write_preserves_previous_archive_and_broadcasts_failure() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let mut game = engine::session::protocol::ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()), 41,
    ).unwrap();
    let update = game.end_civil_day_update().unwrap();
    let previous = native_store::DayEndCandidate::capture(&game, &SaveCandidateKey { seq: update.seq_to, settled_date: update.boundary.settled_date }).unwrap();
    native_store::ArchiveWriter::activate(database.clone(), "current").unwrap().save_day_end(&previous).unwrap();
    let manager = SessionManager::with_database(database.clone());
    let id = manager.resume_session("current").unwrap().unwrap();
    let handles = manager.lookup(&id).unwrap();
    let mut events = handles.subscribe_events();
    database.with_connection(|connection| {
        connection.execute_batch("PRAGMA query_only = ON")?;
        Ok(())
    }).unwrap();
    handles.advance_civil_day(1).await.unwrap();
    let mut updates = Vec::new();
    loop {
        match events.try_recv() {
            Ok(update) => updates.push(update),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty) => break,
            Err(error) => panic!("日终广播读取失败：{error}"),
        }
    }
    let failure = updates.iter().find_map(|update| update.failure.as_ref()).expect("SQLite 写入失败必须广播");
    assert_eq!(failure.code, "DAY_END_ARCHIVE_FAILED");
    assert!(failure.recoverable);
    assert_eq!(serde_json::to_value(database.load("current").unwrap().unwrap()).unwrap(), serde_json::to_value(previous).unwrap());
}
