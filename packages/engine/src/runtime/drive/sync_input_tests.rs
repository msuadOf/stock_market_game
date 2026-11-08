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

/// 只统计真实 copy site；不会把三表写入、采集调用或 getter 次数冒充 clone。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct SyncCopyCosts {
    captured_personals: usize,
    mirror_accounts: usize,
    mirror_personals: usize,
}

#[derive(Clone, Copy)]
pub(super) enum SyncCopy {
    CapturedPersonal,
    MirrorAccount,
    MirrorPersonal,
}
thread_local! {
    static SYNC_COPY_COSTS: std::cell::Cell<Option<SyncCopyCosts>> = const { std::cell::Cell::new(None) };
}
pub(super) fn record_sync_copy(copy: SyncCopy) {
    SYNC_COPY_COSTS.with(|slot| {
        if let Some(mut costs) = slot.get() {
            match copy {
                SyncCopy::CapturedPersonal => costs.captured_personals += 1,
                SyncCopy::MirrorAccount => costs.mirror_accounts += 1,
                SyncCopy::MirrorPersonal => costs.mirror_personals += 1,
            }
            slot.set(Some(costs));
        }
    });
}
struct SyncCopyScope;
impl SyncCopyScope {
    fn enter() -> Self {
        SYNC_COPY_COSTS.with(|slot| {
            assert!(slot.get().is_none(), "copy 见证不允许嵌套");
            slot.set(Some(SyncCopyCosts::default()));
        });
        Self
    }
    fn costs(&self) -> SyncCopyCosts {
        SYNC_COPY_COSTS.with(|slot| slot.get().expect("copy 见证 scope 必须仍在位"))
    }
}
impl Drop for SyncCopyScope {
    fn drop(&mut self) {
        SYNC_COPY_COSTS.with(|slot| slot.set(None));
    }
}

#[test]
fn unconsumed_progress_does_not_copy_accumulated_sync_mirror_inputs() {
    for policy in [
        crate::RuntimeSyncPolicy::DayEnd,
        crate::RuntimeSyncPolicy::OnDemand,
    ] {
        let mut setup = crate::runtime::bridge_tests::setup_fixture();
        setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
        setup.ticks_per_day = 8;
        setup.auction_ticks = 1;
        setup.closing_auction_ticks = 0;
        setup.runtime_sync_policy = policy;
        let mut session = GameSession::new(setup, 42).unwrap();
        let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
        let ids = driver.id_map.clone();
        let full = driver.collect_personal_full().unwrap();
        let personal = full
            .groups
            .iter()
            .flat_map(|(_, rows)| rows.iter())
            .find_map(|(_, _, value)| value.as_ref())
            .unwrap();
        {
            let witness = SyncCopyScope::enter();
            record_sync_copy(SyncCopy::CapturedPersonal);
            let copied = personal.clone();
            assert_eq!(&copied, personal);
            assert_eq!(
                witness.costs().captured_personals,
                1,
                "计数阳性对照必须见证一次真实 clone"
            );
        }
        let copies = SyncCopyScope::enter();
        for tick in 1..=3 {
            driver.force_due_for_test_at(&ids, tick);
            let input = driver.prepare_driver_step(&session).unwrap();
            let counts = driver.pending_sync_inputs_for_test();
            assert!(
                counts.0 > 0 && counts.1 > 0 && counts.2 > 0 && counts.3,
                "先见证账户、个人、调度与计划真实非空，不能跳过权威采集"
            );
            assert_current_values(&mut driver);
            assert_eq!(copies.costs(), SyncCopyCosts::default(),
                "UNCONSUMED_SYNC_MATERIALIZATION：未消费镜像的 Progress 不得 clone 个人整态或物化累计镜像行");
            let prepared = session
                .prepare_runtime_publication(&mut driver, input, tick)
                .unwrap();
            let ack = session
                .validate_runtime_publication(&driver, &prepared)
                .unwrap();
            assert!(ack.is_none(), "延迟策略日内仍是 Progress");
            session.install_runtime_publication(&mut driver, prepared);
            assert_eq!(session.tick(), tick);
            assert_eq!(session.runtime_mirror_origin().source_tick, 0);
            assert_eq!(
                driver.pending_sync_inputs_for_test(),
                counts,
                "Progress 不清累计权威输入"
            );
        }
        let source = driver.mirror_source();
        let pending = driver.pending_sync_values_for_test();
        // 完整静点 getter 与累计输入使用同一 World，但不消费输入或再执行市场拍。
        let full = driver.prepare_flush_driver_projection(&session).unwrap();
        assert_eq!(driver.pending_sync_values_for_test(), pending);
        let expected_attention = full.static_read.as_ref().unwrap().attention.clone();
        let npc_profiles = session.account_strategy_profiles();
        assert_eq!(
            expected_attention.len(),
            driver.id_map.len(),
            "raw profile 完整覆盖含 Player 的真实名册"
        );
        assert!(expected_attention
            .iter()
            .any(|(id, _, _)| *id == AccountId(0)));
        assert!(
            !npc_profiles.contains_key(&AccountId(0)),
            "Player 不拥有 NPC strategy/attention"
        );
        let expected_plans = serde_json::to_value(&full.plans).unwrap();
        driver.flush_now(&mut session).unwrap();
        for (group, rows) in full.accounts.iter().enumerate() {
            for (local, raw) in rows.iter() {
                if let Some(id) = driver.account_by_owner((group as u16, *local)) {
                    let account = session.account(id).unwrap();
                    assert_eq!(account.cash().cents(), raw.cash + raw.buy_purse_held_cents);
                    for (stock, qty) in raw.shares.iter().enumerate() {
                        let code = crate::StockId(stock as u32);
                        assert_eq!(
                            account.position(&code).map_or(0, |position| position.qty()),
                            qty.checked_add(raw.pending_sell_qty[stock]).unwrap()
                        );
                        assert_eq!(
                            account
                                .position(&code)
                                .map_or(0, |position| position.t1_locked()),
                            raw.frozen_today[stock]
                        );
                    }
                }
            }
        }
        for (index, _, value) in &full.personals {
            assert_eq!(
                driver
                    .cached_personal_at(*index)
                    .map(|personal| personal.to_snapshot()),
                Some(value.to_snapshot()),
                "flush 保完整本人 INFO、belief、history、attention 与经历"
            );
        }
        let attention: std::collections::BTreeMap<_, _> = session
            .runtime_attention_state()
            .into_iter()
            .map(|(id, due, seed)| (id, (due, seed)))
            .collect();
        assert_eq!(
            attention,
            expected_attention
                .into_iter()
                .filter(|(id, _, _)| npc_profiles.contains_key(id))
                .map(|(id, due, seed)| (id, (due, seed)))
                .collect(),
            "调度与 RNG 不因延迟复制丢失"
        );
        assert_eq!(
            attention.len(),
            npc_profiles.len(),
            "NPC attention 不得遗漏或多出真实策略账户"
        );
        assert_eq!(
            serde_json::to_value(session.runtime_plans_snapshot()).unwrap(),
            expected_plans
        );
        assert_eq!(driver.mirror_source(), source);
        assert_eq!(
            (session.tick(), session.runtime_mirror_origin().source_tick),
            (3, 3)
        );
        assert_eq!(driver.pending_sync_inputs_for_test().0, 0);
        assert_eq!(driver.pending_sync_inputs_for_test().1, 0);
        assert_eq!(driver.pending_sync_inputs_for_test().2, 0);
    }
}

#[test]
fn mirror_consumers_materialize_once_for_each_policy() {
    for policy in [
        crate::RuntimeSyncPolicy::EveryTick,
        crate::RuntimeSyncPolicy::DayEnd,
        crate::RuntimeSyncPolicy::OnDemand,
    ] {
        let mut setup = crate::runtime::bridge_tests::setup_fixture();
        setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
        setup.ticks_per_day = 2;
        setup.auction_ticks = 1;
        setup.closing_auction_ticks = 0;
        setup.runtime_sync_policy = policy;
        let mut session = GameSession::new(setup, 42).unwrap();
        session
            .fixture_account_mut(&AccountId(0))
            .unwrap()
            .fixture_set_positions(std::collections::BTreeMap::from([(
                crate::StockId(0),
                crate::account::Position::from_restored_parts(100, 100, 100_000, 0),
            )]));
        let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
        let copies = SyncCopyScope::enter();
        for tick in 1..=2 {
            let before = copies.costs();
            let input = driver.prepare_driver_step(&session).unwrap();
            assert!(
                input.personals.is_empty(),
                "本拍已验证个人态须 move 入累计表"
            );
            assert_eq!(copies.costs(), before, "采集不提前复制累计镜像行");
            let counts = driver.pending_sync_inputs_for_test();
            let values = driver.pending_sync_values_for_test();
            let prepared = session
                .prepare_runtime_publication(&mut driver, input, tick)
                .unwrap();
            let flush = policy == crate::RuntimeSyncPolicy::EveryTick || tick == 2;
            let after = copies.costs();
            assert_eq!(after.captured_personals, 0);
            assert_eq!(
                after.mirror_accounts - before.mirror_accounts,
                if flush { counts.0 } else { 0 }
            );
            assert_eq!(
                after.mirror_personals - before.mirror_personals,
                if flush { counts.1 } else { 0 }
            );
            let ack = session
                .validate_runtime_publication(&driver, &prepared)
                .unwrap();
            assert_eq!(ack.is_some(), flush);
            session.install_runtime_publication(&mut driver, prepared);
            if let Some(ack) = ack {
                driver.ack_prepared_mirror(ack);
            }
            if flush {
                for (owner, value) in values.personals {
                    assert_eq!(
                        driver
                            .cached_personal_at(driver.global_index(owner))
                            .map(|personal| personal.to_snapshot()),
                        Some(value)
                    );
                }
                assert_eq!(driver.pending_sync_inputs_for_test().0, 0);
                assert_eq!(driver.pending_sync_inputs_for_test().1, 0);
            } else {
                assert_eq!(driver.pending_sync_values_for_test(), values);
                assert_eq!(session.runtime_mirror_origin().source_tick, 0);
            }
        }
        assert_eq!(
            (session.tick(), session.day(), driver.world_committed_tick()),
            (2, 1, 2)
        );
        assert_eq!(session.runtime_mirror_origin().source_tick, 2);
        assert_eq!(
            session
                .account(AccountId(0))
                .unwrap()
                .position(&crate::StockId(0))
                .unwrap()
                .t1_locked(),
            0
        );
        assert_eq!(driver.publication().load().retained.days.len(), 1);
        session.require_healthy().unwrap();
    }
}

#[test]
fn stale_capture_and_retained_prepare_failure_keep_accumulated_values_for_retry() {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 2;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let mut session = GameSession::new(setup, 42).unwrap();
    let seller = AccountId(1);
    let buyer = AccountId(2);
    let stock = crate::StockId(0);
    session
        .fixture_account_mut(&seller)
        .unwrap()
        .fixture_set_positions(std::collections::BTreeMap::from([(
            stock,
            crate::account::Position::from_restored_parts(100, 0, 100_000, 0),
        )]));
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    let ids = driver.id_map.clone();
    driver.defer_due_for_test_at(&ids, u64::MAX);
    let receipts = [(seller, crate::Side::Sell), (buyer, crate::Side::Buy)]
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (owner, side))| crate::session::ReceiptBearingIntent {
                owner,
                intent: crate::Intent::PlaceLimit {
                    code: stock,
                    side,
                    price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                    qty: 100,
                },
                account_ordinal: 0,
                stock_ordinal: ordinal as u64,
            },
        )
        .collect::<Vec<_>>();
    let upper = driver
        .world_public_views()
        .iter()
        .map(|view| view.daily_upper_limit)
        .collect::<Vec<_>>();
    driver.inject_player_intents(&receipts, &upper).unwrap();
    let first = driver.prepare_driver_step(&session).unwrap();
    let old_source = first.sync_source;
    assert_eq!(
        first
            .events
            .iter()
            .filter(|event| matches!(event,
        Event::Trade { maker, taker, code, qty: 100, price, .. }
            if *maker == seller && *taker == buyer && *code == stock && price.cents() == 1_000))
            .count(),
        1,
        "先见证真实 100 股成交，不能空 fixture 编造 bar"
    );
    let progress = session
        .prepare_runtime_publication(&mut driver, first, 1)
        .unwrap();
    assert!(session
        .validate_runtime_publication(&driver, &progress)
        .unwrap()
        .is_none());
    session.install_runtime_publication(&mut driver, progress);
    let receipt_watermark = session.receipt_watermark();
    let seller_confirmation = session.personal_trade_confirmations(seller).unwrap();
    let buyer_confirmation = session.personal_trade_confirmations(buyer).unwrap();
    assert_eq!(seller_confirmation.len(), 1);
    assert_eq!(buyer_confirmation.len(), 1);
    // ADR-0042 的 maker/taker 为卖腿/买腿；跨组真实到达先后不改变腿方向。
    assert_eq!(
        (
            seller_confirmation[0].side,
            seller_confirmation[0].stock,
            seller_confirmation[0].quantity_shares,
            seller_confirmation[0].price_cents
        ),
        (crate::Side::Sell, stock, 100, 1_000)
    );
    assert_eq!(
        (
            buyer_confirmation[0].side,
            buyer_confirmation[0].stock,
            buyer_confirmation[0].quantity_shares,
            buyer_confirmation[0].price_cents
        ),
        (crate::Side::Buy, stock, 100, 1_000)
    );
    let input = driver.prepare_driver_step(&session).unwrap();
    assert!(input.day_end.is_some());
    assert_ne!(old_source, input.sync_source);
    let counts = driver.pending_sync_inputs_for_test();
    let values = driver.pending_sync_values_for_test();
    assert!(counts.0 > 0 && counts.1 > 0 && counts.2 > 0);
    assert_current_values(&mut driver);
    let snapshot = serde_json::to_value(session.snapshot()).unwrap();
    let source = driver.mirror_source();
    let published = driver.publication().load();
    let mut stale = input.clone();
    stale.sync_source = old_source;
    let error = session
        .prepare_runtime_mirror(&mut driver, stale, 2)
        .err()
        .unwrap();
    assert!(
        error.contains("捕获来源已过期"),
        "必须拒绝旧 capture：{error}"
    );
    assert_eq!(driver.pending_sync_values_for_test(), values);
    let mut corrupt = input.clone();
    let bar = corrupt
        .day_end
        .as_mut()
        .unwrap()
        .bars
        .iter_mut()
        .filter_map(Option::as_mut)
        .flat_map(|bars| bars.iter_mut())
        .find(|bar| bar.volume_shares == 100 && bar.trade_count == 1)
        .expect("真实 100 股成交必须产生对应 minute bar");
    bar.volume_shares = bar.volume_shares.checked_add(1).unwrap();
    assert_eq!(bar.volume_shares, 101);
    let error = session
        .prepare_runtime_mirror(&mut driver, corrupt, 2)
        .err()
        .unwrap();
    assert!(
        error.contains("分钟累计量/额/笔与日K不一致"),
        "必须命中 retained 对账：{error}"
    );
    assert_eq!(driver.pending_sync_inputs_for_test(), counts);
    assert_eq!(driver.pending_sync_values_for_test(), values);
    assert_eq!(driver.mirror_source(), source);
    assert_eq!(serde_json::to_value(session.snapshot()).unwrap(), snapshot);
    assert!(std::sync::Arc::ptr_eq(
        &published.retained,
        &driver.publication().load().retained
    ));
    session.require_healthy().unwrap();
    let starts = driver.world_report_log().len();
    let prepared = session
        .prepare_runtime_mirror(&mut driver, input, 2)
        .unwrap();
    let ack = session
        .validate_runtime_mirror_source(&driver, &prepared)
        .unwrap();
    session.install_runtime_mirror(&mut driver, prepared);
    driver.ack_prepared_mirror(ack);
    assert_eq!(
        (session.tick(), session.day(), driver.world_committed_tick()),
        (2, 1, 2)
    );
    assert_eq!(
        driver.world_report_log().len(),
        starts,
        "重试只消费原输入，不再 Start"
    );
    assert_eq!(driver.publication().load().retained.days.len(), 1);
    assert_eq!(driver.pending_sync_inputs_for_test().0, 0);
    assert_eq!(driver.pending_sync_inputs_for_test().1, 0);
    assert_eq!(
        session.personal_trade_confirmations(seller).unwrap().len(),
        1,
        "retained 重试不重发卖腿确认"
    );
    assert_eq!(
        session.personal_trade_confirmations(buyer).unwrap().len(),
        1,
        "retained 重试不重发买腿确认"
    );
    assert_eq!(
        session.personal_trade_confirmations(seller).unwrap(),
        seller_confirmation
    );
    assert_eq!(
        session.personal_trade_confirmations(buyer).unwrap(),
        buyer_confirmation
    );
    assert_eq!(session.receipt_watermark(), receipt_watermark);
    session.require_healthy().unwrap();
}

#[test]
fn deferred_dayend_trade_and_nonempty_plan_survive_flush_and_day_boundary() {
    let mut setup = crate::runtime::bridge_tests::setup_fixture();
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 3;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let mut session = GameSession::new(setup, 42).unwrap();
    let seller = AccountId(1);
    let buyer = AccountId(2);
    let stock = crate::StockId(0);
    let expected_buyer_qty = session
        .account(buyer)
        .unwrap()
        .position(&stock)
        .map_or(0, |position| position.qty())
        .checked_add(100)
        .unwrap();
    session
        .fixture_account_mut(&seller)
        .unwrap()
        .fixture_set_positions(std::collections::BTreeMap::from([(
            stock,
            crate::account::Position::from_restored_parts(100, 0, 100_000, 0),
        )]));
    let mut plans = session.runtime_plans_snapshot().clone();
    plans
        .create(crate::plans::PlanOpen {
            account: seller,
            code: crate::StockId(1),
            direction: crate::Side::Buy,
            target: crate::plans::PlanTarget::ShareCount(100),
            opinion: crate::plans::PlanOpinion {
                signal_score_bp: 3000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8000,
            urgency: crate::plans::Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    assert!(plans
        .active_investor_codes(seller)
        .any(|code| *code == crate::StockId(1)));
    session.set_runtime_plans(plans);
    let mut driver = RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    let ids = driver.id_map.clone();
    driver.defer_due_for_test_at(&ids, u64::MAX);
    let receipts = [(seller, crate::Side::Sell), (buyer, crate::Side::Buy)]
        .into_iter()
        .enumerate()
        .map(
            |(ordinal, (owner, side))| crate::session::ReceiptBearingIntent {
                owner,
                intent: crate::Intent::PlaceLimit {
                    code: stock,
                    side,
                    price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                    qty: 100,
                },
                account_ordinal: 0,
                stock_ordinal: ordinal as u64,
            },
        )
        .collect::<Vec<_>>();
    let upper = driver
        .world_public_views()
        .iter()
        .map(|view| view.daily_upper_limit)
        .collect::<Vec<_>>();
    driver.inject_player_intents(&receipts, &upper).unwrap();
    let input = driver
        .prepare_driver_step_mode(&session, true, true)
        .unwrap();
    assert!(input.events.iter().any(|event| matches!(event,
        Event::Trade { maker, taker, code, qty: 100, price, .. }
        if *maker == seller && *taker == buyer && *code == stock && price.cents() == 1_000)));
    let prepared = session
        .prepare_runtime_publication(&mut driver, input, 1)
        .unwrap();
    assert!(session
        .validate_runtime_publication(&driver, &prepared)
        .unwrap()
        .is_none());
    session.install_runtime_publication(&mut driver, prepared);

    let full = driver.prepare_flush_driver_projection(&session).unwrap();
    let expected_plans = serde_json::to_value(&full.plans).unwrap();
    driver.flush_now(&mut session).unwrap();
    assert_eq!(
        serde_json::to_value(session.runtime_plans_snapshot()).unwrap(),
        expected_plans
    );
    assert!(session
        .runtime_plans_snapshot()
        .active_investor_codes(seller)
        .any(|code| *code == crate::StockId(1)));
    for (group, rows) in full.accounts.iter().enumerate() {
        for (local, raw) in rows.iter() {
            if let Some(id) = driver.account_by_owner((group as u16, *local)) {
                let account = session.account(id).unwrap();
                assert_eq!(account.cash().cents(), raw.cash + raw.buy_purse_held_cents);
                for (index, qty) in raw.shares.iter().enumerate() {
                    let code = crate::StockId(index as u32);
                    assert_eq!(
                        account.position(&code).map_or(0, |position| position.qty()),
                        qty.checked_add(raw.pending_sell_qty[index]).unwrap()
                    );
                }
            }
        }
    }
    assert_eq!(
        session
            .account(seller)
            .unwrap()
            .position(&stock)
            .map_or(0, |p| p.qty()),
        0
    );
    assert_eq!(
        session
            .account(buyer)
            .unwrap()
            .position(&stock)
            .unwrap()
            .qty(),
        expected_buyer_qty
    );
    let seller_cash = session.account(seller).unwrap().cash();
    let buyer_cash = session.account(buyer).unwrap().cash();
    for tick in 2..=3 {
        let input = driver
            .prepare_driver_step_mode(&session, true, true)
            .unwrap();
        let prepared = session
            .prepare_runtime_publication(&mut driver, input, tick)
            .unwrap();
        let ack = session
            .validate_runtime_publication(&driver, &prepared)
            .unwrap();
        session.install_runtime_publication(&mut driver, prepared);
        if let Some(ack) = ack {
            driver.ack_prepared_mirror(ack);
        }
    }
    assert_eq!((session.tick(), session.day()), (3, 1));
    assert_eq!(session.account(seller).unwrap().cash(), seller_cash);
    assert_eq!(session.account(buyer).unwrap().cash(), buyer_cash);
    assert_eq!(
        session
            .account(buyer)
            .unwrap()
            .position(&stock)
            .unwrap()
            .qty(),
        expected_buyer_qty
    );
    assert!(session
        .runtime_plans_snapshot()
        .active_investor_codes(seller)
        .any(|code| *code == crate::StockId(1)));
    assert_eq!(session.runtime_mirror_origin().source_tick, 3);
}
