#[path = "../../engine/tests/publications/session_fixture.rs"]
mod fixture;

use engine::session::protocol::{ProtocolSession, SaveCandidateKey};
use engine::CivilDate;
use native_store::{ArchiveWriter, DayEndCandidate, NativeDatabase};

fn candidate() -> DayEndCandidate {
    let mut game = ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()), 41,
    ).unwrap();
    let update = game.end_civil_day_update().unwrap();
    DayEndCandidate::capture(&game, &SaveCandidateKey {
        seq: update.seq_to,
        settled_date: update.boundary.settled_date,
    }).unwrap()
}

#[test]
fn archive_load_and_copy_reject_active_minutes_without_rewriting_facts() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    writer.save_day_end(&candidate()).unwrap();
    let mut slot = database.load("current").unwrap().unwrap();
    let code = slot.setup.stocks[0].code.clone();
    slot.runtime_state.active_minute_history.insert(code, vec![engine::session::MinuteBar {
        minute_of_day: 570, phase: engine::session::HistoryTradingPhase::Continuous,
        open: engine::Money::from_cents(1000), high: engine::Money::from_cents(1000),
        low: engine::Money::from_cents(1000), close: engine::Money::from_cents(1000),
        volume_shares: 100, turnover_cents: 100000, trade_count: 1,
    }]);
    let payload = serde_json::to_string(&slot).unwrap();
    database.with_connection(|connection| {
        connection.execute("UPDATE archive_slots SET payload = ?1 WHERE slot_id = 'current'", [&payload])?;
        Ok(())
    }).unwrap();
    assert!(database.load("current").is_err());
    assert!(database.copy("current", "copied", "不应创建").is_err());
    assert!(database.load("copied").unwrap().is_none());
    let unchanged: String = database.with_connection(|connection| {
        Ok(connection.query_row("SELECT payload FROM archive_slots WHERE slot_id = 'current'", [], |row| row.get(0))?)
    }).unwrap();
    assert_eq!(unchanged, payload);
}

fn capture_current(game: &ProtocolSession) -> DayEndCandidate {
    let saved = game.save().unwrap();
    DayEndCandidate::capture(game, &SaveCandidateKey { seq: saved.snapshot.seq, settled_date: saved.civil_clock.settled_through.unwrap() }).unwrap()
}

#[test]
fn real_archive_reopens_and_restores_complete_day_end() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../.tmp/native-store-tests");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("reopen-{}-{:?}.sqlite", std::process::id(), std::thread::current().id()));
    let slot = candidate();
    {
        let database = NativeDatabase::open(&path).unwrap();
        let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
        writer.save_day_end(&slot).unwrap();
        assert_eq!(database.list().unwrap().len(), 1);
    }
    {
        let database = NativeDatabase::open(&path).unwrap();
        let saved = database.load("current").unwrap().unwrap();
        assert_eq!(serde_json::to_value(&saved).unwrap(), serde_json::to_value(&slot).unwrap());
        ProtocolSession::restore(&saved).unwrap();
        assert_eq!(database.journal_mode().unwrap(), "delete");
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn slot_management_is_transactional_and_does_not_reload_the_game() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    let slot = candidate();
    writer.save_day_end(&slot).unwrap();
    database.copy("current", "manual", "周末档").unwrap();
    database.rename("manual", "另一槽").unwrap();
    assert_eq!(database.list().unwrap().len(), 2);
    assert_eq!(database.list().unwrap()[1].name, "另一槽");
    assert!(database.copy("current", "manual", "重复").is_err());
    assert!(database.rename("missing", "不存在").is_err());
    database.delete("manual").unwrap();
    assert!(database.load("manual").unwrap().is_none());
    assert_eq!(serde_json::to_value(database.load("current").unwrap().unwrap()).unwrap(), serde_json::to_value(slot).unwrap());
}

#[test]
fn replaced_timeline_and_failed_write_preserve_previous_archive() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let old_writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    let slot = candidate();
    old_writer.save_day_end(&slot).unwrap();
    let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    assert!(old_writer.save_day_end(&slot).is_err());
    database.with_connection(|connection| {
        connection.execute_batch("PRAGMA query_only = ON")?;
        Ok(())
    }).unwrap();
    assert!(writer.save_day_end(&slot).is_err());
    assert_eq!(serde_json::to_value(database.load("current").unwrap().unwrap()).unwrap(), serde_json::to_value(slot).unwrap());
}

#[test]
fn intraday_checkpoint_and_corrupt_current_structure_are_rejected() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    let mut game = ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap()), 41,
    ).unwrap();
    game.step_frame().unwrap();
    assert!(DayEndCandidate::capture(&game, &SaveCandidateKey { seq: game.seq(), settled_date: game.civil_date() }).is_err());
    assert!(database.list().unwrap().is_empty());
    writer.save_day_end(&candidate()).unwrap();
    database.with_connection(|connection| {
        connection.execute("UPDATE archive_slots SET payload = json_set(payload, '$.schema_version', 1)", [])?;
        Ok(())
    }).unwrap();
    assert!(database.load("current").is_err());
}

#[test]
fn existing_unrecognized_schema_is_not_migrated() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/native-store-tests");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("old-schema-{}-{:?}.sqlite", std::process::id(), std::thread::current().id()));
    let connection = native_store::rusqlite::Connection::open(&path).unwrap();
    connection.execute_batch("CREATE TABLE old_archive(payload TEXT)").unwrap();
    drop(connection);
    assert!(NativeDatabase::open(&path).is_err());
    std::fs::remove_file(path).unwrap();
}

#[test]
fn switching_archive_target_invalidates_previous_target_writer() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    let slot = candidate();
    writer.save_day_end(&slot).unwrap();
    let selected = writer.renew(Some("other")).unwrap();
    assert!(writer.save_day_end(&slot).is_err());
    selected.save_day_end(&slot).unwrap();
    assert_eq!(database.list().unwrap().len(), 2);
}

#[test]
fn identity_and_market_control_remain_independent_from_market_archive() {
    let database = NativeDatabase::open_in_memory().unwrap();
    database.create_subject_with_credential(&native_store::identities::SubjectRecord {
        subject_id: "subject-controller".into(), username: None, password_hash: None,
    }, "credential-controller").unwrap();
    database.grant_market_control("shared-market", "subject-controller").unwrap();
    let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    writer.save_day_end(&candidate()).unwrap();
    database.delete("current").unwrap();
    assert_eq!(database.market_controllers("shared-market").unwrap(), vec!["subject-controller"]);
    assert!(database.subject_by_credential_hash("credential-controller").unwrap().is_some());
    assert!(database.grant_market_control("shared-market", "missing-subject").is_err());
}

#[test]
fn failed_timeline_commit_keeps_previous_archive_writer_valid() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    let mut game = ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()), 41,
    ).unwrap();
    game.end_civil_day_update().unwrap();
    writer.save_day_end(&capture_current(&game)).unwrap();
    assert!(writer.renew_after(None, || Err(native_store::StoreError::Invalid("更换 ingress 失败".into()))).is_err());
    game.end_civil_day_update().unwrap();
    writer.save_day_end(&capture_current(&game)).unwrap();
    assert_eq!(database.load("current").unwrap().unwrap().snapshot.seq, game.save().unwrap().snapshot.seq);
}

#[test]
fn explicit_slot_selection_physically_reopens_without_changing_economic_payloads() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/native-store-tests");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("selection-{}-{:?}.sqlite", std::process::id(), std::thread::current().id()));
    {
        let database = NativeDatabase::open(&path).unwrap();
        let slot = candidate();
        ArchiveWriter::activate(database.clone(), "current").unwrap().save_day_end(&slot).unwrap();
        database.copy("current", "manual", "手动档").unwrap();
        database.select("manual").unwrap();
        for slot_id in ["current", "manual"] {
            assert_eq!(serde_json::to_value(database.load(slot_id).unwrap().unwrap()).unwrap(), serde_json::to_value(&slot).unwrap());
        }
    }
    {
        let database = NativeDatabase::open(&path).unwrap();
        assert_eq!(database.selection().unwrap(), native_store::ArchiveSelection::Selected("manual".into()));
        database.delete("manual").unwrap();
        assert_eq!(database.selection().unwrap(), native_store::ArchiveSelection::Cleared);
        assert!(database.load("current").unwrap().is_some());
    }
    std::fs::remove_file(path).unwrap();
}

#[test]
fn failed_selection_preserves_previous_selected_slot() {
    let database = NativeDatabase::open_in_memory().unwrap();
    assert_eq!(database.selection().unwrap(), native_store::ArchiveSelection::Uninitialized);
    ArchiveWriter::activate(database.clone(), "current").unwrap().save_day_end(&candidate()).unwrap();
    database.copy("current", "manual", "手动档").unwrap();
    assert!(database.select("missing").is_err());
    database.with_connection(|connection| { connection.execute_batch("PRAGMA query_only = ON")?; Ok(()) }).unwrap();
    assert!(database.select("manual").is_err());
    assert_eq!(database.selection().unwrap(), native_store::ArchiveSelection::Selected("current".into()));
}

#[test]
fn deleting_active_slot_prepares_new_target_without_selecting_an_existing_archive() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    writer.save_day_end(&candidate()).unwrap();
    database.copy("current", "existing", "其他旧档").unwrap();
    let fresh_writer = writer.delete_slot("current", "fresh-after-delete").unwrap();
    assert_eq!(database.selection().unwrap(), native_store::ArchiveSelection::Cleared);
    assert!(database.load("fresh-after-delete").unwrap().is_none());
    assert!(writer.save_day_end(&candidate()).is_err());
    fresh_writer.save_day_end(&candidate()).unwrap();
    assert_eq!(database.selection().unwrap(), native_store::ArchiveSelection::Selected("fresh-after-delete".into()));
    assert!(database.load("existing").unwrap().is_some());
}

#[test]
fn independent_physical_open_and_inode_alias_cannot_create_second_writer() {
    let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.tmp/native-store-tests");
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join(format!("single-writer-{}-{:?}.sqlite", std::process::id(), std::thread::current().id()));
    let alias = path.with_extension("alias.sqlite");
    let database = NativeDatabase::open(&path).unwrap();
    std::fs::hard_link(&path, &alias).unwrap();
    assert!(NativeDatabase::open(&path).is_err(), "第二物理连接必须显式拒绝，不能生成另一份 generation 1");
    assert!(NativeDatabase::open(&alias).is_err(), "同 inode 路径别名不能绕过独占 writer");
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "exclusive_file_lock_child", "--ignored", "--test-threads=1"])
        .env("NATIVE_STORE_LOCK_TEST_PATH", &path)
        .output().unwrap();
    assert!(child.status.success(), "第二进程必须显式拒绝同 SQLite writer：{}", String::from_utf8_lossy(&child.stderr));
    let clone = database.clone();
    drop(database);
    assert!(NativeDatabase::open(&path).is_err(), "最后一个 clone 释放前不能提前释放 writer 锁");
    drop(clone);
    NativeDatabase::open(&path).unwrap();
    std::fs::remove_file(alias).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[test]
#[ignore]
fn exclusive_file_lock_child() {
    let path = std::env::var_os("NATIVE_STORE_LOCK_TEST_PATH").expect("仅由独占锁父用例传入物理路径");
    assert!(NativeDatabase::open(std::path::Path::new(&path)).is_err());
}

#[test]
fn ordinary_snapshot_after_completed_day_cannot_replace_immutable_day_end() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    let mut game = ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap()), 41,
    ).unwrap();
    for _ in 0..fixture::TICKS_PER_DAY { game.step_frame().unwrap(); }
    game.end_civil_day_update().unwrap();
    writer.save_day_end(&capture_current(&game)).unwrap();
    let before = database.load("current").unwrap().unwrap();
    game.step_frame().unwrap();
    let ordinary = game.game().save().unwrap();
    assert!(ProtocolSession::restore(&ordinary).is_err());
    assert!(DayEndCandidate::capture(&game, &SaveCandidateKey { seq: ordinary.snapshot.seq, settled_date: ordinary.civil_clock.settled_through.unwrap() }).is_err());
    assert_eq!(serde_json::to_value(database.load("current").unwrap().unwrap()).unwrap(), serde_json::to_value(before).unwrap());
}

#[test]
fn typed_candidate_capture_only_reads_successful_immutable_civil_update() {
    let mut setup = fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap());
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let mut game = ProtocolSession::new(setup, 41).unwrap();
    let unavailable = SaveCandidateKey { seq: 0, settled_date: CivilDate::from_iso("2030-01-02").unwrap() };
    assert!(native_store::DayEndCandidate::capture(&game, &unavailable).is_err());
    for _ in 0..fixture::TICKS_PER_DAY { game.step_frame().unwrap(); }
    let update = game.end_civil_day_update().unwrap();
    let expected = game.save().unwrap();
    game.step_frame().unwrap();
    let key = SaveCandidateKey { seq: update.seq_to, settled_date: update.boundary.settled_date };
    let candidate = native_store::DayEndCandidate::capture(&game, &key).unwrap();
    assert_eq!(serde_json::to_value(candidate.as_slot()).unwrap(), serde_json::to_value(expected).unwrap());
    assert!(game.game().save().unwrap().snapshot.tick > candidate.as_slot().snapshot.tick);
}

#[test]
fn stale_writer_cannot_renew_or_commit_a_replacement_timeline() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let old_writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    let current_writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    let committed = std::cell::Cell::new(false);
    for target in [None, Some("other-target")] {
        assert!(old_writer.renew_after(target, || { committed.set(true); Ok(()) }).is_err(), "失效 owner 不得抢占当前 writer 或另一个目标");
        assert!(!committed.get(), "拒绝旧 owner 必须先于 ingress／时间线替换");
    }
    current_writer.save_day_end(&candidate()).unwrap();
    assert!(database.load("current").unwrap().is_some());
    assert!(database.load("other-target").unwrap().is_none());
}

#[test]
fn stale_writer_cannot_mutate_other_archive_metadata() {
    let database = NativeDatabase::open_in_memory().unwrap();
    let old_writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    old_writer.save_day_end(&candidate()).unwrap();
    database.copy("current", "other", "其他档").unwrap();
    let current_writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    assert!(old_writer.rename_slot("other", "旧请求改名").is_err());
    assert!(old_writer.copy_slot("other", "stale-copy", "旧请求复制").is_err());
    assert!(old_writer.delete_slot("other", "unused-target").is_err());
    assert_eq!(database.list().unwrap().len(), 2);
    assert_eq!(database.list().unwrap()[1].name, "其他档");
    current_writer.save_day_end(&candidate()).unwrap();
}
