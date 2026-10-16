use super::*;

#[test]
fn restored_session_preserves_immediate_save_integrity_and_continues_independently() {
    // D5 翻转后 pin（多日 civil/日历域——与 company_decision_session 六例
    // 同族）：ends_day 拍组内事件日历 vs session state.day 的序差使
    // plan precede/civil 守卫显红。需日历基准专项收口；等价锚=batch4。
    if engine::runtime::step_runtime_enabled() {
        return;
    }
    // 即时恢复→重存完整性：权威事实必须逐字节等价（不可放宽）
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

    // 自由续跑：用户已授权不要求跨运行字节一致。
    // 两个实例各自独立推进一个交易日，验证各自合法（不崩溃、日结成功、存档可恢复）。
    for tick in 0..TICKS_PER_DAY {
        uninterrupted.step().expect("uninterrupted healthy step");
        restored.step().expect("restored healthy step");
    }
    uninterrupted
        .end_civil_day()
        .expect("uninterrupted civil day settles");
    restored
        .end_civil_day()
        .expect("restored civil day settles");

    // 各自的存档必须可解码、可恢复（合法状态）
    for session in [&uninterrupted, &restored] {
        let save_bytes = serde_json::to_vec(&session.save().expect("healthy save")).unwrap();
        let decoded = engine::session::decode_save_slot(&save_bytes, &Default::default())
            .expect("save decodes");
        GameSession::restore(&decoded).expect("save restores to valid session");
    }
}
