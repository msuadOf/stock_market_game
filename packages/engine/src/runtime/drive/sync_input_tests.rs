//! E1 生产覆盖输入红测；真实 World 收束不安装镜像，验证跨拍保留与有界。
use super::*;

#[derive(Debug, Default, PartialEq)]
pub(super) struct SyncValuesForTest {
    pub(super) accounts:
        std::collections::BTreeMap<(u16, u32), std::sync::Arc<super::super::group::GroupAccount>>,
    pub(super) personals:
        std::collections::BTreeMap<(u16, u32), super::super::world::BeliefParticipantSnapshot>,
    pub(super) attentions: std::collections::BTreeMap<(u16, u32), (u64, u64)>,
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
    assert_eq!(
        actual, expected,
        "三表逐 owner 的最后整态必须等于当前 World，不能只更新计数与 source_seq"
    );
}

/// 复用同一生产 candidate 的 prepare→source验证→move install，不扩大 Session 方法可见性。
fn install_prepared_tick(session: &mut GameSession, driver: &mut RuntimeDriver) {
    let tick = driver.world_committed_tick() + 1;
    let input = driver.prepare_driver_step(session).unwrap();
    let prepared = session.prepare_runtime_mirror(driver, input, tick).unwrap();
    let ack = session
        .validate_runtime_mirror_source(driver, &prepared)
        .unwrap();
    session.install_runtime_mirror(driver, prepared);
    driver.ack_prepared_mirror(ack);
    assert_eq!(session.tick(), tick);
    assert_eq!(driver.world_committed_tick(), tick);
    assert!(driver.sync_inputs.has_successful_baseline());
    assert_eq!(driver.sync_inputs.accounts().len(), 0);
    assert_eq!(driver.sync_inputs.beliefs().len(), 0);
    assert_eq!(driver.sync_inputs.attentions().len(), 0);
}

fn capture_window(ticks: u64) {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 64;
    setup.auction_ticks = 1;
    setup.closing_auction_ticks = 0;
    let mut session = GameSession::new(setup, 42).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    assert_eq!(
        driver.id_map.len(),
        3,
        "Player 加两个真实 NPC，尾组有 padding"
    );
    let expected_personals = session.runtime_personal_states().len();
    assert_eq!(expected_personals, 2);
    let attention_count: usize = driver
        .collect_attention()
        .unwrap()
        .iter()
        .map(|(_, rows)| rows.len())
        .sum();
    assert_eq!(attention_count, 3, "profile 仅覆盖真实名册");
    let before = serde_json::to_value(session.snapshot()).unwrap();
    let ids = driver.id_map.clone();
    let mut previous_source_seq = 0;
    let mut last_input = None;
    for tick in 1..=ticks {
        driver.force_due_for_test_at(&ids, tick);
        let input = driver.prepare_driver_step(&session).unwrap();
        if tick == 1 {
            assert!(!input.accounts.is_empty(), "首拍实际取得完整账户输入");
        }
        let (accounts, personals, attentions, plans, source_seq) =
            driver.pending_sync_inputs_for_test();
        assert_eq!(
            accounts, 3,
            "完整首拍账户输入必须跨未安装拍保留，padding 不占覆盖键"
        );
        assert_eq!(personals, expected_personals, "两位本人最新整态跨拍保留");
        assert_eq!(
            attentions, attention_count,
            "真实 profile 调度与 RNG 必须进入第三表"
        );
        assert!(plans, "完整计划待读取标记不能在 prepare 时清除");
        assert!(
            source_seq > previous_source_seq,
            "私有 source_seq 每次真实输入 checked 前进"
        );
        previous_source_seq = source_seq;
        assert_current_values(&mut driver);
        assert!(accounts + personals + attentions <= 3 * driver.id_map.len());
        assert_eq!(driver.world_committed_tick(), tick);
        assert_eq!(
            driver.account_writes_for_test(),
            0,
            "收束输入不提前安装账户"
        );
        assert_eq!(driver.personal_installs_for_test(), 0);
        assert_eq!(serde_json::to_value(session.snapshot()).unwrap(), before);
        last_input = Some(input);
    }
    let mut input = last_input.unwrap();
    input.emit_price_tick = false;
    let prepared = session
        .prepare_runtime_mirror(&mut driver, input, ticks)
        .unwrap();
    let ack = session
        .validate_runtime_mirror_source(&driver, &prepared)
        .unwrap();
    session.install_runtime_mirror(&mut driver, prepared);
    driver.ack_prepared_mirror(ack);
    assert_eq!(
        driver.pending_sync_inputs_for_test(),
        (0, 0, 0, false, previous_source_seq),
        "唯一成功 mirror swap 清三表与 PlansPending，私有 source_seq 不回拨"
    );
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
    assert_eq!(
        before.0, 3,
        "失败前必须已有非空真实输入，不能以空表自证保留"
    );
    assert!(
        before.1 > 0 && before.2 > 0 && before.3,
        "拒绝前个人、调度与 PlansPending 必须实际非空"
    );
    assert_current_values(&mut driver);
    let before_values = driver.pending_sync_values_for_test();
    session.inject_runtime_install_failure();
    let rejected = session.prepare_runtime_mirror(&mut driver, input, 1);
    assert!(rejected.is_err());
    assert_eq!(
        driver.pending_sync_inputs_for_test(),
        before,
        "失败 prepare 不清三表或 PlansPending"
    );
    assert_eq!(
        driver.pending_sync_values_for_test(),
        before_values,
        "失败不只保数量，也须保完整 owner/值"
    );
    assert_eq!(driver.account_writes_for_test(), 0);
    assert_eq!(driver.personal_installs_for_test(), 0);
}

#[test]
fn day_end_source_sequence_exhaustion_precedes_world_start() {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 2;
    setup.auction_ticks = 1;
    setup.closing_auction_ticks = 0;
    let mut session = GameSession::new(setup, 42).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    install_prepared_tick(&mut session, &mut driver);
    driver.sync_source_seq = u64::MAX - 1;
    let source = driver.mirror_source();
    let values = driver.pending_sync_values_for_test();
    let counts = driver.pending_sync_inputs_for_test();
    let error = driver
        .prepare_driver_step(&session)
        .err()
        .expect("日界需两次登记，MAX-1 不得先 Start");
    assert!(error.contains("source_seq"), "{error}");
    assert_eq!(driver.mirror_source(), source);
    assert_eq!(driver.world_committed_tick(), 1);
    assert_eq!(driver.pending_sync_values_for_test(), values);
    assert_eq!(driver.pending_sync_inputs_for_test(), counts);
}

#[test]
fn ordinary_source_sequence_exhaustion_precedes_world_start() {
    let mut session = GameSession::new(crate::runtime::bridge_tests::setup_fixture(), 42).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    driver.sync_source_seq = u64::MAX;
    let source = driver.mirror_source();
    let error = driver
        .prepare_driver_step(&session)
        .err()
        .expect("耗尽不得 Start");
    assert!(error.contains("source_seq"), "{error}");
    assert_eq!(driver.mirror_source(), source);
    assert_eq!(driver.world_committed_tick(), 0);
    assert_eq!(
        driver.pending_sync_inputs_for_test(),
        (0, 0, 0, false, u64::MAX)
    );
}

#[test]
fn manual_full_source_sequence_exhaustion_precedes_day_end() {
    let mut session = GameSession::new(crate::runtime::bridge_tests::setup_fixture(), 42).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    driver.sync_source_seq = u64::MAX;
    let source = driver.mirror_source();
    let error = driver
        .install_manual_day_end(&mut session, false)
        .unwrap_err();
    assert!(error.contains("source_seq"), "{error}");
    assert_eq!(driver.mirror_source(), source);
    assert_eq!(driver.world_committed_tick(), 0);
    assert_eq!(
        driver.pending_sync_inputs_for_test(),
        (0, 0, 0, false, u64::MAX)
    );
}

#[test]
fn sparse_capture_after_successful_flush_preserves_failed_candidate_inputs() {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    let mut session = GameSession::new(setup, 42).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    install_prepared_tick(&mut session, &mut driver);
    assert!(driver.sync_inputs.has_successful_baseline());
    let after_swap_seq = driver.sync_source_seq;
    assert_eq!(
        driver.pending_sync_inputs_for_test(),
        (0, 0, 0, false, after_swap_seq)
    );
    let ids = driver.id_map.clone();
    driver.force_due_for_test_at(&ids, 2);
    let input = driver.prepare_driver_step(&session).unwrap();
    let counts = driver.pending_sync_inputs_for_test();
    let values = driver.pending_sync_values_for_test();
    assert!(
        counts.1 > 0 && counts.2 > 0 && counts.3,
        "成功基线后的真实 sparse 个人/调度输入必须非空"
    );
    assert!(counts.0 <= 3 && counts.1 <= 3 && counts.2 <= 3);
    assert!(counts.4 > after_swap_seq);
    let current_personals: std::collections::BTreeMap<_, _> = driver
        .world
        .collect_personal_full()
        .unwrap()
        .groups
        .into_iter()
        .flat_map(|(group, rows)| {
            rows.into_iter()
                .filter_map(move |(local, _, value)| value.map(|value| ((group, local), value)))
        })
        .collect();
    let current_attention: std::collections::BTreeMap<_, _> = driver
        .world
        .collect_group_attention()
        .unwrap()
        .into_iter()
        .flat_map(|(group, rows)| {
            rows.into_iter()
                .map(move |(local, due, seed)| ((group, local), (due, seed)))
        })
        .collect();
    for (owner, row) in &values.personals {
        assert_eq!(Some(row), current_personals.get(owner));
    }
    for (owner, row) in &values.attentions {
        assert_eq!(Some(row), current_attention.get(owner));
    }
    session.inject_runtime_install_failure();
    assert!(session
        .prepare_runtime_mirror(&mut driver, input, 2)
        .is_err());
    assert_eq!(driver.pending_sync_inputs_for_test(), counts);
    assert_eq!(
        driver.pending_sync_values_for_test(),
        values,
        "已接管 sparse 输入在 candidate 拒绝后保完整值"
    );
}

#[test]
fn static_full_getters_preserve_sync_tables_and_reject_changed_source() {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    let mut session = GameSession::new(setup, 42).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    driver.adopt_session_seq(&session);
    let before = driver.pending_sync_inputs_for_test();
    let input = driver.prepare_static_driver_projection(&session).unwrap();
    let repeated = driver.prepare_static_driver_projection(&session).unwrap();
    assert_eq!(input.accounts, repeated.accounts);
    assert_eq!(input.personals, repeated.personals);
    assert_eq!(
        serde_json::to_value(&input.plans).unwrap(),
        serde_json::to_value(&repeated.plans).unwrap()
    );
    assert_eq!(
        input.static_read.as_ref().unwrap().source,
        repeated.static_read.as_ref().unwrap().source
    );
    assert_eq!(
        input.static_read.as_ref().unwrap().attention,
        repeated.static_read.as_ref().unwrap().attention
    );
    assert_eq!(
        input.static_read.as_ref().unwrap().resting,
        repeated.static_read.as_ref().unwrap().resting
    );
    assert_eq!(
        input.static_read.as_ref().unwrap().serials,
        repeated.static_read.as_ref().unwrap().serials
    );
    let first = session
        .prepare_runtime_mirror(&mut driver, input.clone(), session.tick())
        .unwrap();
    let _second = session
        .prepare_runtime_mirror(&mut driver, repeated, session.tick())
        .unwrap();
    assert_eq!(
        driver.pending_sync_inputs_for_test(),
        before,
        "Full getter 不登记 source_seq、不消费或清空三表"
    );
    let owner = driver.account_by_owner((1, 0)).unwrap();
    driver.defer_due_for_test_at(&[owner], 99);
    assert!(
        session
            .prepare_runtime_mirror(&mut driver, input, session.tick())
            .is_err(),
        "getter 后来源变更不能把旧 payload 重贴成新来源"
    );
    assert!(session
        .validate_runtime_mirror_source(&driver, &first)
        .is_err());
    assert_eq!(driver.pending_sync_inputs_for_test(), before);
}

#[test]
fn static_full_rejects_session_revision_changes_before_prepare_and_swap() {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    let mut session = GameSession::new(setup, 42).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    driver.adopt_session_seq(&session);
    let input = driver.prepare_static_driver_projection(&session).unwrap();
    let first = session
        .prepare_runtime_mirror(&mut driver, input.clone(), session.tick())
        .unwrap();
    let source = driver.mirror_source();
    let pending = driver.pending_sync_inputs_for_test();
    let account = driver.account_by_owner((1, 0)).unwrap();
    let cash = session.account(account).unwrap().cash().cents();
    session
        .fixture_account_mut(&account)
        .unwrap()
        .fixture_set_cash(crate::Money::from_cents(cash + 17));
    assert_eq!(
        driver.mirror_source(),
        source,
        "本负控只改变 Session revision，不能靠 World 改源拒绝"
    );
    assert!(session
        .prepare_runtime_mirror(&mut driver, input, session.tick())
        .is_err());
    assert!(session
        .validate_runtime_mirror_source(&driver, &first)
        .is_err());
    assert_eq!(driver.pending_sync_inputs_for_test(), pending);
    assert_eq!(session.account(account).unwrap().cash().cents(), cash + 17);
}
