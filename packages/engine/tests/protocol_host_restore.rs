#[path = "publications/session_fixture.rs"]
mod fixture;

use engine::session::protocol::ProtocolSession;
use engine::CivilDate;

#[test]
fn batch_retains_every_frame_and_one_final_player_delta() {
    let setup = fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap());
    let mut session = ProtocolSession::new(setup, 41).unwrap();
    let frames: Vec<_> = (0..3).map(|_| session.step_frame().unwrap()).collect();
    let expected: Vec<_> = frames
        .iter()
        .map(|frame| serde_json::to_value(frame).unwrap())
        .collect();

    let batch = session.tick_batch(frames).unwrap();

    batch.validate().unwrap();
    assert_eq!(batch.frames.len(), 3);
    for (frame, expected) in batch.frames.iter().zip(expected) {
        assert_eq!(serde_json::to_value(frame).unwrap(), expected);
    }
    assert!(batch.runtime_snapshot.is_none());
    let delta = batch.runtime_delta.as_ref().unwrap();
    assert_eq!(delta.tick, 3);
    assert_eq!(delta.seq_from, batch.frames.first().unwrap().seq_from);
    assert_eq!(delta.seq_to, batch.frames.last().unwrap().seq_to);
    assert_eq!(delta.seq_to, session.game().seq());
    assert_eq!(delta.day, session.game().day());
    assert_eq!(delta.phase, session.game().phase());
    assert_eq!(delta.accounts.len(), 1);
    assert!(delta.accounts.contains_key(&engine::AccountId(0)));
    assert!(delta.working_orders.reset);
}

#[test]
fn malformed_batch_is_a_typed_fatal() {
    let setup = fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap());
    let session = ProtocolSession::new(setup, 41).unwrap();

    let result = session.tick_batch(Vec::new());

    assert!(matches!(
        result,
        Err(engine::session::StepFatal::InvariantViolation { .. })
    ));
}

#[test]
fn intraday_checkpoint_preserves_engine_cursor_but_is_not_a_public_day_end_save() {
    let setup = fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap());
    let mut original = ProtocolSession::new(setup, 41).unwrap();
    original.step_frame().unwrap();
    let saved = original.game().save().unwrap();

    let restored = engine::GameSession::restore(&saved).unwrap();

    assert_eq!(restored.tick(), original.game().tick());
    assert_eq!(restored.seq(), original.game().seq());
    assert!(matches!(ProtocolSession::restore(&saved),
        Err(engine::SessionError::InvalidSave(reason))
        if reason == "公共日级档必须来自完整的自然日日终结算"));
    assert_eq!(
        serde_json::to_value(original.game().save().unwrap()).unwrap(),
        serde_json::to_value(saved).unwrap(),
        "rejected public restore must not mutate its source"
    );
}

// Intraday protocol restoration is an explicit verification capability, not
// the public day-end loading API. Retain its missing-history rollback coverage.
#[cfg(feature = "verification-harness")]
#[test]
fn verification_checkpoint_refuses_missing_closing_history_without_advancing_cursor() {
    let setup = fixture::civil_setup(CivilDate::from_iso("2030-01-02").unwrap());
    let mut original = ProtocolSession::new(setup, 41).unwrap();
    original.step_frame().unwrap();
    let saved = original.game().save().unwrap();
    let mut restored = ProtocolSession::restore_verification_checkpoint(&saved).unwrap();
    assert_eq!(restored.game().tick(), original.game().tick());
    assert_eq!(restored.game().seq(), original.game().seq());
    for _ in 1..fixture::TICKS_PER_DAY {
        restored.step_frame().unwrap();
    }
    let before = restored.game().snapshot();
    assert!(restored.end_civil_day_update().is_err());
    assert_eq!(restored.game().tick(), before.tick);
    assert_eq!(restored.game().seq(), before.seq);
}
