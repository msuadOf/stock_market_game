//! 稳定计划 writer 映射的成本与运行期变更拒绝边界。

use super::*;

fn fixture() -> (GameSession, RuntimeDriver) {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 64;
    setup.auction_ticks = 1;
    setup.closing_auction_ticks = 0;
    let mut session = GameSession::new(setup, 42).unwrap();
    session.set_runtime_plans(crate::runtime::plan_identity_tests::seed_book());
    let driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    (session, driver)
}

#[test]
fn real_driver_ticks_reuse_owner_map_without_rebuilding_it() {
    let (session, mut driver) = fixture();
    let builds = crate::runtime::plan_ownership::owner_map_builds_for_test();
    assert!(builds > 0, "计数必须位于真实 BTreeMap 构造点");
    for tick in 1..=3 {
        driver.prepare_driver_step(&session).unwrap();
        assert_eq!(driver.world_committed_tick(), tick);
    }
    assert_eq!(
        crate::runtime::plan_ownership::owner_map_builds_for_test(),
        builds,
        "OWNER_MAP_REUSE：稳定名册的真实 Driver ticks 不得重建 owner map"
    );
}

fn rejects_change_before_start(change: impl FnOnce(&mut RuntimeDriver)) {
    let (session, mut driver) = fixture();
    // 先保留真实未安装输入；空表不足以证明错误路径没有消费或覆盖旧值。
    let ids = driver.id_map.clone();
    driver.force_due_for_test_at(&ids, 1);
    driver.prepare_driver_step(&session).unwrap();
    let counts = driver.pending_sync_inputs_for_test();
    assert!(
        counts.0 > 0 && counts.1 > 0 && counts.2 > 0 && counts.3,
        "{counts:?}"
    );
    let values = driver.pending_sync_values_for_test();
    let cache = driver.administrative_cache_snapshot_for_test();
    let session_before = serde_json::to_value(session.snapshot()).unwrap();
    let plans = driver.world.collect_plans().unwrap();
    assert!(plans
        .iter()
        .any(|(_, book)| book.plan_ids().next().is_some()));
    let accounts = driver.world.snapshot_accounts().unwrap();
    let attention = driver.world.collect_group_attention().unwrap();
    change(&mut driver);
    let error = driver
        .prepare_driver_step(&session)
        .err()
        .expect("OWNER_LAYOUT_PREFLIGHT：运行期计划名册或组布局变化必须在 Start 前显式拒绝");
    assert!(
        error.contains("计划名册或组布局已改变"),
        "OWNER_LAYOUT_PREFLIGHT：{error}"
    );
    assert_eq!(driver.world_committed_tick(), 1, "拒绝不得发出下一 Start");
    assert_eq!(driver.administrative_cache_snapshot_for_test(), cache);
    assert_eq!(driver.pending_sync_inputs_for_test(), counts);
    assert_eq!(driver.pending_sync_values_for_test(), values);
    assert_eq!(driver.world.collect_plans().unwrap(), plans);
    assert_eq!(driver.world.snapshot_accounts().unwrap(), accounts);
    assert_eq!(driver.world.collect_group_attention().unwrap(), attention);
    assert_eq!(
        serde_json::to_value(session.snapshot()).unwrap(),
        session_before
    );
}

#[test]
fn changed_owner_is_rejected_before_start_without_losing_pending_values() {
    rejects_change_before_start(|driver| driver.id_map[1] = AccountId(999));
}

#[test]
fn reordered_roster_is_rejected_before_start_without_losing_pending_values() {
    rejects_change_before_start(|driver| driver.id_map.swap(0, 2));
}

#[test]
fn duplicate_roster_is_rejected_before_start_without_losing_pending_values() {
    rejects_change_before_start(|driver| driver.id_map[1] = driver.id_map[0]);
}

#[test]
fn shortened_roster_is_rejected_before_start_without_losing_pending_values() {
    rejects_change_before_start(|driver| {
        driver.id_map.pop();
    });
}

#[test]
fn changed_group_width_is_rejected_before_start_without_losing_pending_values() {
    rejects_change_before_start(|driver| driver.accounts_per_group += 1);
}

#[test]
fn changed_roster_is_rejected_by_static_flush_manual_day_end_and_administrative_reads() {
    let (mut session, mut driver) = fixture();
    let cache = driver.administrative_cache_snapshot_for_test();
    let before = serde_json::to_value(session.snapshot()).unwrap();
    driver.id_map.swap(0, 2);
    let errors = [
        driver
            .prepare_static_driver_projection(&session)
            .err()
            .expect("静点必须拒绝陈旧名册"),
        driver.flush_now(&mut session).unwrap_err(),
        driver
            .install_manual_day_end(&mut session, false)
            .unwrap_err(),
        driver
            .prepare_administrative_baseline(&session)
            .err()
            .expect("行政基线必须拒绝陈旧名册")
            .to_string(),
    ];
    for error in errors {
        assert!(error.contains("计划名册或组布局已改变"), "{error}");
    }
    assert_eq!(driver.administrative_cache_snapshot_for_test(), cache);
    assert_eq!(serde_json::to_value(session.snapshot()).unwrap(), before);
    assert_eq!(driver.world_committed_tick(), 0);
}
