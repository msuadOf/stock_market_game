//! E1 生产覆盖输入红测；真实 World 收束不安装镜像，验证跨拍保留与有界。
use super::*;

#[derive(Debug, Default, PartialEq)]
pub(super) struct SyncValuesForTest {
    accounts: std::collections::BTreeMap<(u16, u32), std::sync::Arc<super::super::group::GroupAccount>>,
    personals: std::collections::BTreeMap<(u16, u32), super::super::world::BeliefParticipantSnapshot>,
    attentions: std::collections::BTreeMap<(u16, u32), (u64, u64)>,
}

/// 另走当前 World 完整只读接口作值对照；不会运行第二次 tick 或清 sparse 输入。
fn assert_current_values(driver: &mut RuntimeDriver) {
    let actual = driver.pending_sync_values_for_test();
    let mut expected = SyncValuesForTest::default();
    for (group, rows) in driver.world.snapshot_accounts().unwrap() {
        for (local, row) in rows.into_iter().enumerate() {
            let owner = (group, local as u32);
            if driver.account_by_owner(owner).is_some() {
                expected.accounts.insert(owner, std::sync::Arc::new(row));
            }
        }
    }
    for (group, rows) in driver.world.collect_personal_full().unwrap().groups {
        for (local, _, value) in rows {
            if let Some(value) = value {
                expected.personals.insert((group, local), value);
            }
        }
    }
    for (group, rows) in driver.world.collect_group_attention().unwrap() {
        for (local, due, seed) in rows {
            expected.attentions.insert((group, local), (due, seed));
        }
    }
    assert_eq!(actual, expected, "三表逐 owner 的最后整态必须等于当前 World，不能只更新计数与 source_seq");
}

fn capture_window(ticks: u64) {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 64;
    setup.auction_ticks = 1;
    setup.closing_auction_ticks = 0;
    let mut session = GameSession::new(setup, 42).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    assert_eq!(driver.id_map.len(), 3, "Player 加两个真实 NPC，尾组有 padding");
    let expected_personals = session.runtime_personal_states().len();
    assert_eq!(expected_personals, 2);
    let attention_count: usize = driver.collect_attention().unwrap().iter().map(|(_, rows)| rows.len()).sum();
    assert_eq!(attention_count, 3, "profile 仅覆盖真实名册");
    let before = serde_json::to_value(session.snapshot()).unwrap();
    let ids = driver.id_map.clone();
    let mut previous_source_seq = 0;
    for tick in 1..=ticks {
        driver.force_due_for_test_at(&ids, tick);
        let input = driver.prepare_driver_step(&session).unwrap();
        if tick == 1 { assert!(!input.accounts.is_empty(), "首拍实际取得完整账户输入"); }
        let (accounts, personals, attentions, plans, source_seq) = driver.pending_sync_inputs_for_test();
        assert_eq!(accounts, 3, "完整首拍账户输入必须跨未安装拍保留，padding 不占覆盖键");
        assert_eq!(personals, expected_personals, "两位本人最新整态跨拍保留");
        assert_eq!(attentions, attention_count, "真实 profile 调度与 RNG 必须进入第三表");
        assert!(plans, "完整计划待读取标记不能在 prepare 时清除");
        assert!(source_seq > previous_source_seq, "私有 source_seq 每次真实输入 checked 前进");
        previous_source_seq = source_seq;
        assert_current_values(&mut driver);
        assert!(accounts + personals + attentions <= 3 * driver.id_map.len());
        assert_eq!(driver.world_committed_tick(), tick);
        assert_eq!(driver.account_writes_for_test(), 0, "收束输入不提前安装账户");
        assert_eq!(driver.personal_installs_for_test(), 0);
        assert_eq!(serde_json::to_value(session.snapshot()).unwrap(), before);
    }
}

#[test]
fn production_sync_tables_retain_n_uninstalled_ticks() {
    capture_window(4);
}

#[test]
fn production_sync_tables_retain_2n_uninstalled_ticks() {
    capture_window(8);
}

#[test]
fn rejected_mirror_prepare_preserves_production_sync_inputs() {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    let mut session = GameSession::new(setup, 42).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    let ids = driver.id_map.clone();
    driver.force_due_for_test_at(&ids, 1);
    let input = driver.prepare_driver_step(&session).unwrap();
    let before = driver.pending_sync_inputs_for_test();
    assert_eq!(before.0, 3, "失败前必须已有非空真实输入，不能以空表自证保留");
    assert!(before.1 > 0 && before.2 > 0 && before.3, "拒绝前个人、调度与 PlansPending 必须实际非空");
    assert_current_values(&mut driver);
    let before_values = driver.pending_sync_values_for_test();
    session.inject_runtime_install_failure();
    let rejected = session.prepare_runtime_mirror(&mut driver, input, 1);
    assert!(rejected.is_err());
    assert_eq!(driver.pending_sync_inputs_for_test(), before, "失败 prepare 不清三表或 PlansPending");
    assert_eq!(driver.pending_sync_values_for_test(), before_values, "失败不只保数量，也须保完整 owner/值");
    assert_eq!(driver.account_writes_for_test(), 0);
    assert_eq!(driver.personal_installs_for_test(), 0);
}
