use super::*;

#[path = "../../../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

#[tokio::test]
async fn desktop_successful_closed_day_writes_native_archive_without_client_save() {
    let database = native_store::NativeDatabase::open_in_memory().unwrap();
    let game = ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()), 41,
    ).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game, false, PausePreferences::default(), "sqlite", "sqlite",
    );
    harness.actor.archive = Some(native_store::ArchiveWriter::activate(database.clone(), "current").unwrap());
    assert!(database.load("current").unwrap().is_none());
    harness.actor.run_cycle(1);
    let slot = database.load("current").unwrap().unwrap();
    assert_eq!(slot.civil_clock.settled_through, Some(CivilDate::from_iso("2030-01-06").unwrap()));
    assert_eq!(serde_json::to_value(&slot).unwrap(), serde_json::to_value(harness.actor.game.save().unwrap()).unwrap());
}

#[tokio::test]
async fn desktop_failed_write_preserves_archive_and_emits_recoverable_failure() {
    let database = native_store::NativeDatabase::open_in_memory().unwrap();
    let mut game = ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()), 41,
    ).unwrap();
    let update = game.end_civil_day_update().unwrap();
    let previous = native_store::DayEndCandidate::capture(&game, &engine::session::protocol::SaveCandidateKey { seq: update.seq_to, settled_date: update.boundary.settled_date }).unwrap();
    let writer = native_store::ArchiveWriter::activate(database.clone(), "current").unwrap();
    writer.save_day_end(&previous).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game, false, PausePreferences::default(), "sqlite", "sqlite",
    );
    harness.actor.archive = Some(writer);
    harness.subscribe_failures();
    database.with_connection(|connection| {
        connection.execute_batch("PRAGMA query_only = ON")?;
        Ok(())
    }).unwrap();
    harness.actor.run_cycle(1);
    let failure: serde_json::Value = serde_json::from_str(&harness.subscribe_failures().try_recv().unwrap()).unwrap();
    assert_eq!(failure["code"], "DAY_END_ARCHIVE_FAILED");
    assert_eq!(failure["recoverable"], true);
    assert!(!harness.actor.pacing.is_running());
    assert!(!harness.actor.cmd_rx.is_closed());
    assert_eq!(serde_json::to_value(database.load("current").unwrap().unwrap()).unwrap(), serde_json::to_value(previous).unwrap());
}

#[tokio::test]
async fn desktop_trading_day_only_persists_after_full_close_and_never_reloads_intraday() {
    let database = native_store::NativeDatabase::open_in_memory().unwrap();
    let mut setup = fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let game = ProtocolSession::new(setup, 41).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(game, false, PausePreferences::default(), "trading-archive", "trading-archive");
    harness.actor.archive = Some(native_store::ArchiveWriter::activate(database.clone(), "current").unwrap());
    for _ in 0..fixture::TICKS_PER_DAY - 1 {
        harness.actor.run_cycle(1);
        assert!(database.load("current").unwrap().is_none());
    }
    harness.actor.run_cycle(1);
    let saved = database.load("current").unwrap().unwrap();
    assert_eq!(saved.civil_clock.settled_through, Some(CivilDate::from_iso("2030-01-02").unwrap()));
    assert!(saved.resting_orders.values().all(Vec::is_empty));
    assert!(saved.auction_orders.values().all(Vec::is_empty));
    database.with_connection(|connection| { connection.execute("UPDATE archive_slots SET payload='corrupt external edit'", [])?; Ok(()) }).unwrap();
    harness.actor.run_cycle(1);
    assert_eq!(harness.actor.game.tick(), fixture::TICKS_PER_DAY + 1);
    assert!(database.load("current").is_err());
    assert_eq!(harness.actor.game.save().unwrap().snapshot.tick, fixture::TICKS_PER_DAY);
}
