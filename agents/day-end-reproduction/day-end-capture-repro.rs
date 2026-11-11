#[path = "../../engine/tests/publications/session_fixture.rs"]
mod fixture;

use engine::session::protocol::{ProtocolSession, SaveCandidateKey};
use engine::CivilDate;
use native_store::{ArchiveWriter, DayEndCandidate, NativeDatabase};

#[test]
fn without_automatic_capture_an_earlier_published_candidate_is_available() {
    let mut game = ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()),
        41,
    )
    .unwrap();
    let first = game.end_civil_day_update().unwrap();
    game.end_civil_day_update().unwrap();
    let slot = game
        .save_candidate(&SaveCandidateKey {
            seq: first.seq_to,
            settled_date: first.boundary.settled_date,
        })
        .expect("未消费的旧日候选应可捕获");
    assert_eq!(slot.snapshot.seq, first.seq_to);
}

#[test]
fn another_clients_baseline_preserves_an_earlier_published_candidate() {
    let mut game = ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()),
        41,
    )
    .unwrap();
    let first = game.end_civil_day_update().unwrap();
    game.end_civil_day_update().unwrap();
    game.prepare_public_baseline();
    let slot = game
        .save_candidate(&SaveCandidateKey {
            seq: first.seq_to,
            settled_date: first.boundary.settled_date,
        })
        .expect("另一客户端取得 baseline 不应清除已发布给原客户端的候选");
    assert_eq!(slot.snapshot.seq, first.seq_to);
}

#[test]
fn server_archive_capture_preserves_an_earlier_published_candidate_for_the_client() {
    let mut game = ProtocolSession::new(
        fixture::civil_setup(CivilDate::from_iso("2030-01-05").unwrap()),
        41,
    )
    .unwrap();
    let database = NativeDatabase::open_in_memory().unwrap();
    let writer = ArchiveWriter::activate(database.clone(), "current").unwrap();
    let first = game.end_civil_day_update().unwrap();
    let key = SaveCandidateKey {
        seq: first.seq_to,
        settled_date: first.boundary.settled_date,
    };
    let first_slot = DayEndCandidate::capture(&game, &key).unwrap();
    writer.save_day_end(&first_slot).unwrap();
    // 首日仍为最新候选时可以重复捕获；下一自然日后只剩 pending 才能回应旧 key。
    assert_eq!(game.save_candidate(&key).unwrap().snapshot.seq, first.seq_to);
    let second = game.end_civil_day_update().unwrap();
    let second_slot = DayEndCandidate::capture(
        &game,
        &SaveCandidateKey {
            seq: second.seq_to,
            settled_date: second.boundary.settled_date,
        },
    )
    .unwrap();
    writer.save_day_end(&second_slot).unwrap();
    assert_eq!(
        database.load("current").unwrap().unwrap().snapshot.seq,
        second.seq_to
    );
    let delayed_file_capture = game
        .save_candidate(&key)
        .expect("Server 自动存档不应消费稍后显式文件导出仍需要的旧日候选");
    assert_eq!(delayed_file_capture.snapshot.seq, first.seq_to);
}
