#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use engine::session::protocol::ProtocolSession;
use engine::AccountId;
use server::actor::PublicBaselineSnapshot;

#[test]
fn public_baseline_preserves_mirror_origin_and_only_player_account() {
    let mut setup = fixture::civil_setup(
        engine::CivilDate::from_iso("2030-01-02").expect("测试日期应有效"),
    );
    setup.npcs.retail_count = 2;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.history_len = 1;

    let game = ProtocolSession::new(setup, 41).expect("测试 ProtocolSession 初始化失败");
    let mut snapshot = game.snapshot();
    let expected_origin = serde_json::to_value(&snapshot.mirror_origin)
        .expect("测试 Snapshot mirror_origin 序列化失败");
    assert!(snapshot.accounts.contains_key(&AccountId(0)));
    // 仅测试 DTO 过滤的合成输入，不冒充真实 NPC 公开投影。
    let synthetic_non_player = snapshot
        .accounts
        .get(&AccountId(0))
        .expect("真实 Snapshot 应包含 Player 账户")
        .clone();
    assert!(snapshot
        .accounts
        .insert(AccountId(1), synthetic_non_player)
        .is_none());
    assert!(
        snapshot
            .accounts
            .keys()
            .any(|account_id| *account_id != AccountId(0)),
        "测试 DTO 应包含非 Player 账户以验证不会泄漏"
    );

    let baseline = PublicBaselineSnapshot::from(snapshot);
    let actual_account_ids = baseline.accounts.keys().copied().collect::<Vec<_>>();
    assert_eq!(actual_account_ids, vec![AccountId(0)]);

    let baseline_json = serde_json::to_value(&baseline)
        .expect("PublicBaselineSnapshot 序列化失败");
    let actual_origin = baseline_json
        .get("mirror_origin")
        .expect("PublicBaselineSnapshot JSON 缺少 mirror_origin");

    assert_eq!(actual_origin, &expected_origin);
    let origin_object = actual_origin
        .as_object()
        .expect("mirror_origin JSON 应为对象");
    assert_eq!(origin_object.len(), 7, "mirror_origin 应完整包含七个字段");
    for field in [
        "authority_epoch",
        "install_sequence",
        "source_tick",
        "source_generation",
        "non_tick_revision",
    ] {
        let value = origin_object
            .get(field)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or_else(|| panic!("mirror_origin.{field} 应为非负整数"));
        assert!(
            value <= 9_007_199_254_740_991,
            "mirror_origin.{field} 超出 JavaScript 安全整数范围"
        );
    }
}
