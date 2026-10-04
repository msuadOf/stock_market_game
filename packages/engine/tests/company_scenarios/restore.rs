use super::*;

#[test]
fn restored_and_uninterrupted_sessions_keep_canonical_events_and_saves_identical() {
    // Given: two real twins split by a mid-scenario save after a trading day and civil settlement.
    let mut uninterrupted = representative_session(&fixture().start_date);
    run_trading_day(&mut uninterrupted);
    uninterrupted
        .end_civil_day()
        .expect("first civil day settles");
    let bytes = serde_json::to_vec(&uninterrupted.save().expect("healthy save"))
        .expect("authoritative save serializes");
    let decoded =
        engine::session::decode_save_slot(&bytes, &Default::default()).expect("save decodes");
    let mut restored = GameSession::restore(&decoded).expect("save restores");
    assert_eq!(
        bytes,
        serde_json::to_vec(&restored.save().expect("healthy save")).unwrap()
    );

    // When: both twins receive exactly the same market commands through one complete trading day.
    for tick in 0..TICKS_PER_DAY {
        let original_events = uninterrupted.step().expect("healthy step");
        let restored_events = restored.step().expect("healthy step");
        assert_eq!(
            serde_json::to_vec(&original_events).unwrap(),
            serde_json::to_vec(&restored_events).unwrap(),
            "tick {tick}: canonical events diverged"
        );
    }
    uninterrupted
        .end_civil_day()
        .expect("uninterrupted civil day settles");
    restored
        .end_civil_day()
        .expect("restored civil day settles");

    // 恢复后全部权威存档状态必须逐字节一致，不能只比较可见快照。
    assert_eq!(
        serde_json::to_vec(&uninterrupted.save().expect("healthy save")).unwrap(),
        serde_json::to_vec(&restored.save().expect("healthy save")).unwrap()
    );
}
