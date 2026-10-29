use super::*;

/// 该夹具直接走现有生产安装入口；不创建第二个 Session 或重跑 World。
fn short_runtime_session() -> GameSession {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 4;
    setup.auction_ticks = 1;
    setup.closing_auction_ticks = 0;
    GameSession::new(setup, 42).unwrap()
}

#[test]
fn candidate_validation_changes_no_session_domain() {
    let mut game = short_runtime_session();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    let accounts = format!("{:?}", game.runtime_accounts_snapshot());
    let personal = format!("{:?}", game.runtime_belief_states());
    let attention = game.runtime_attention_state();
    let plans = format!("{:?}", game.state.plans);
    let mirror = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
    let before_account_writes = driver.account_writes_for_test();
    let before_personal_writes = driver.personal_installs_for_test();
    game.inject_runtime_install_failure();

    let fatal = game.runtime_step_with_install(&mut driver).unwrap_err();

    assert!(fatal.to_string().contains("安装失败注入"));
    assert_eq!(driver.world_committed_tick(), 1, "已发布 World 不退回");
    assert_eq!(format!("{:?}", game.runtime_accounts_snapshot()), accounts);
    assert_eq!(format!("{:?}", game.runtime_belief_states()), personal);
    assert_eq!(game.runtime_attention_state(), attention);
    assert_eq!(format!("{:?}", game.state.plans), plans);
    assert_eq!(
        serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
        mirror
    );
    assert_eq!(game.poison_reason(), Some(&fatal));
    assert!(matches!(game.save(), Err(crate::SessionError::Step(actual)) if actual == fatal));
    assert_eq!(
        driver.account_writes_for_test(),
        before_account_writes,
        "candidate 校验前不得写账户后再撤销"
    );
    assert_eq!(
        driver.personal_installs_for_test(),
        before_personal_writes,
        "candidate 校验前不得写个人态后再撤销"
    );
}

#[test]
fn civil_failure_after_swap_is_terminal_without_restore() {
    let mut game = short_runtime_session();
    game.fixture_account_mut(&AccountId(0))
        .unwrap()
        .fixture_set_positions(std::collections::BTreeMap::from([(
            StockId(0),
            crate::account::Position::from_restored_parts(100, 100, 100_000, 10_000),
        )]));
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    for _ in 0..4 {
        game.runtime_step_with_install(&mut driver).unwrap();
    }
    assert_eq!(game.day(), 1, "真实日结已经安装");
    assert_eq!(
        game.account(AccountId(0)).unwrap().positions()[&StockId(0)].t1_locked(),
        0,
        "真实日结已经解冻非零 T+1 持仓"
    );
    let installed = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
    let retained = driver.publication().load();
    let date = game.civil_date();
    let fatal = StepFatal::InvariantViolation {
        location: "runtime_sync_tests::civil_callback".to_owned(),
        description: "日结安装后最后 civil callback 失败".to_owned(),
    };

    let failed: Result<(), SessionError> =
        game.with_prepared_civil_day(|_, _| Err(SessionError::Step(fatal.clone())));

    assert!(matches!(failed, Err(SessionError::Step(ref error)) if error == &fatal));
    assert_eq!(game.civil_date(), date, "失败 civil 不推进");
    assert_eq!(game.day(), 1, "已安装交易日不能恢复旧值");
    assert_eq!(driver.world_committed_tick(), 4);
    let published = driver.publication().load();
    assert_eq!(
        (published.tick, published.generation),
        (retained.tick, retained.generation)
    );
    assert!(
        std::sync::Arc::ptr_eq(&published.retained, &retained.retained),
        "已发布 retained 不撤回"
    );
    assert_eq!(
        serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
        installed,
        "已安装账户、T+1、K 与行情保持同一读点"
    );
    assert_eq!(game.poison_reason(), Some(&fatal), "失败必须封存同一首错");
    assert!(matches!(game.save(), Err(crate::SessionError::Step(actual)) if actual == fatal));
    assert_eq!(game.step().unwrap_err(), fatal);
    assert!(matches!(game.end_civil_day(), Err(SessionError::Step(ref error)) if error == &fatal));
}

#[test]
fn ordinary_callback_failure_keeps_old_mirror_and_first_error() {
    let mut game = short_runtime_session();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    let before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
    let fatal = StepFatal::InvariantViolation {
        location: "runtime_sync_tests::last_callback".to_owned(),
        description: "私有 candidate 最后 callback 拒绝".to_owned(),
    };
    let failed: Result<(), StepFatal> =
        game.runtime_step_with_prepared_mirror(&mut driver, |projection, facts| {
            assert_eq!(projection.tick(), 1, "callback 读取已准备的新拍");
            projection
                .project_frame(facts)?
                .validate()
                .map_err(|error| StepFatal::InvariantViolation {
                    location: "runtime_sync_tests::projection".to_owned(),
                    description: error.to_string(),
                })?;
            Err(fatal.clone())
        });
    assert_eq!(failed.unwrap_err(), fatal);
    assert_eq!(driver.world_committed_tick(), 1);
    assert_eq!(driver.account_writes_for_test(), 0);
    assert_eq!(driver.personal_installs_for_test(), 0);
    assert_eq!(
        serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
        before
    );
    assert_eq!(game.poison_reason(), Some(&fatal));
    assert_eq!(driver.step_frame_equivalent(&mut game).unwrap_err(), fatal);
    let later = StepFatal::InvariantViolation {
        location: "later".to_owned(),
        description: "不得覆盖首错".to_owned(),
    };
    assert_eq!(game.poison_runtime_step(later), fatal);
    assert!(matches!(game.save(), Err(crate::SessionError::Step(actual)) if actual == fatal));
}

#[test]
fn constructing_driver_without_install_does_not_poison_civil_callback_rejection() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 3).unwrap();
    setup.npcs.inst_count = 0;
    let mut game = GameSession::new(setup, 42).unwrap();
    let _driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    assert!(game.state.runtime_resting_snapshot.is_none());
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    let fatal = StepFatal::InvariantViolation {
        location: "uninstalled_civil".to_owned(),
        description: "未安装市场的 civil 业务拒绝".to_owned(),
    };
    let failed: Result<(), SessionError> =
        game.with_prepared_civil_day(|_, _| Err(SessionError::Step(fatal.clone())));
    assert!(matches!(failed, Err(SessionError::Step(ref actual)) if actual == &fatal));
    assert!(game.poison_reason().is_none());
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
}

#[test]
fn legacy_delivery_accepts_real_hot_strategy_without_belief_participant() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 1;
    let mut game = GameSession::new(setup, 42).unwrap();
    assert!(game.account(AccountId(1)).unwrap().strategy().is_some());
    assert!(!game.runtime_has_belief_participant(AccountId(1)));
    let mut driver =
        crate::runtime::drive::RuntimeDriver::from_session_grouped(&mut game, 2).unwrap();
    assert_eq!(driver.deliver_information_for_test(&mut game).unwrap(), 0);
}

#[test]
fn legacy_delivery_rejects_required_participant_missing_after_construction() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.npcs.retail_count = 1;
    let mut game = GameSession::new(setup, 42).unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    assert!(game
        .account(AccountId(1))
        .unwrap()
        .strategy()
        .unwrap()
        .retail_style()
        .is_some());
    assert!(game
        .state
        .belief_participants
        .remove(&AccountId(1))
        .is_some());
    let error = driver.deliver_information_for_test(&mut game).unwrap_err();
    assert!(
        error.contains("实际策略要求个人态但 participant 缺失"),
        "{error}"
    );
    assert_eq!(driver.world_committed_tick(), 0, "非法投递不得先推进市场");
}

#[test]
fn price_tick_execution_phase_keeps_existing_transition_policy() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.ticks_per_day = 6;
    setup.auction_ticks = 3;
    setup.closing_auction_ticks = 1;
    setup.npcs.inst_count = 0;
    let mut game = GameSession::new(setup, 42).unwrap();
    let mut actual = Vec::new();
    for _ in 0..5 {
        let frame = game.step_frame().unwrap();
        frame.validate().unwrap();
        for fact in &frame.facts {
            if let Event::PriceTick { tick, .. } = &fact.event {
                actual.push(*tick);
            }
        }
    }
    assert_eq!(
        actual,
        vec![4, 5],
        "PriceTick 按本拍执行阶段；快照进度不重写已执行阶段"
    );
    assert_eq!(game.phase(), TradingPhase::ClosingAuction);
}

/// 下一片复用真实 step_frame callback；不新增伪造 Full 或 retained hook。
#[test]
fn day_end_callback_failure_keeps_published_retained_and_old_mirror() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 4;
    setup.auction_ticks = 1;
    setup.closing_auction_ticks = 0;
    let mut game = GameSession::new(setup, 42).unwrap();
    for _ in 0..3 {
        game.step_via_runtime().unwrap();
    }
    let publication = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication();
    let before = publication.load();
    let mirror = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
    let fatal = StepFatal::InvariantViolation {
        location: "runtime_sync_tests::day_end_callback".to_owned(),
        description: "retained 发布后最后 callback 拒绝 Full candidate".to_owned(),
    };

    let failed: Result<((), _), StepFatal> =
        game.step_frame_and_commit_via_runtime(|projection, facts| {
            assert_eq!(projection.tick(), 4);
            projection
                .project_frame(facts)?
                .validate()
                .map_err(|error| StepFatal::InvariantViolation {
                    location: "runtime_sync_tests::day_end_projection".to_owned(),
                    description: error.to_string(),
                })?;
            Err(fatal.clone())
        });

    assert_eq!(failed.unwrap_err(), fatal);
    let published = publication.load();
    assert_eq!(published.tick, 4, "普通 tick 已发布，不回退");
    assert_eq!(
        published.retained.days.len(),
        before.retained.days.len() + 1,
        "F1 保留已经发布的真实日结 retained"
    );
    assert_eq!(
        serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
        mirror,
        "F1 candidate 尚未 swap，镜像保持旧来源"
    );
    assert_eq!(
        game.poison_reason(),
        Some(&fatal),
        "F1 必须 terminal，不重放已发布市场"
    );
    assert!(
        game.runtime_driver.is_some(),
        "保留已提交 World 及首错诊断，不能丢弃后重建"
    );
    assert!(matches!(game.save(), Err(crate::SessionError::Step(actual)) if actual == fatal));
    assert_eq!(game.step_frame().unwrap_err(), fatal);
}

#[test]
fn full_personal_read_covers_three_real_groups_without_consuming_sparse_state() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 5;
    setup.npcs.retail_count = 1;
    let mut game = GameSession::new(setup, 42).unwrap();
    let expected: BTreeSet<_> = game.runtime_personal_states().keys().copied().collect();
    let mut driver =
        crate::runtime::drive::RuntimeDriver::from_session_grouped(&mut game, 3).unwrap();
    assert_eq!(driver.id_map.len(), 7);
    assert_eq!(driver.accounts_per_group(), 3);
    let first = driver.collect_personal_full().unwrap();
    assert_eq!(
        first
            .groups
            .iter()
            .map(|(_, rows)| rows.len())
            .collect::<Vec<_>>(),
        [3, 3, 1]
    );
    let sparse_before = driver.collect_group_belief_for_test().unwrap();
    let second = driver.collect_personal_full().unwrap();
    assert_eq!(second.source, first.source);
    assert_eq!(
        second.groups, first.groups,
        "重复读取保完整七字段且不推进个人权威"
    );
    let sparse_after = driver.collect_group_belief_for_test().unwrap();
    assert_eq!(sparse_after, sparse_before, "Full 不消费 dirty 或 pending");
    let owners: BTreeSet<_> = first
        .groups
        .iter()
        .flat_map(|(_, rows)| rows)
        .filter_map(|(_, id, value)| match value {
            Some(value) => {
                assert_eq!(value.information.owner(), *id);
                assert_eq!(value.belief.npc(), *id);
                Some(*id)
            }
            None => {
                assert_eq!(*id, AccountId(0), "本夹具只有真实 Player 明确无个人能力");
                None
            }
        })
        .collect();
    assert_eq!(owners, expected);
}

/// T04 的两臂取同一次 World 收束，强制 Full 只改变读取方式，不再执行本人计算。
#[test]
fn same_world_static_point_normal_and_forced_full_prepare_agree() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 5;
    setup.npcs.retail_count = 1;
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 8;
    setup.auction_ticks = 3;
    setup.closing_auction_ticks = 0;
    let mut game = GameSession::new(setup, 42).unwrap();
    let mut driver =
        crate::runtime::drive::RuntimeDriver::from_session_grouped(&mut game, 3).unwrap();
    game.runtime_step_with_install(&mut driver).unwrap();
    let ids = driver.id_map.clone();
    driver.defer_due_for_test_at(&ids, 99);
    driver.adopt_session_seq(&game);
    let deferred: BTreeSet<_> = game.runtime_personal_states().keys().copied().collect();
    let actual_due: BTreeMap<_, _> = driver
        .collect_attention()
        .unwrap()
        .into_iter()
        .flat_map(|(group, rows)| {
            rows.into_iter()
                .filter_map(|(local, due, _)| {
                    driver.account_by_owner((group, local)).map(|id| (id, due))
                })
                .collect::<Vec<_>>()
        })
        .collect();
    for id in &deferred {
        assert_eq!(
            actual_due[id], 99,
            "先核组真实 profile，不能用错同值两臂自证"
        );
    }
    let input = driver.prepare_driver_step(&game).unwrap();
    assert!(input.day_end.is_none());
    let source = driver.mirror_source();
    let account_writes = driver.account_writes_for_test();
    let personal_writes = driver.personal_installs_for_test();
    let before = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
    let normal = game
        .prepare_runtime_mirror(&mut driver, input.clone(), 2)
        .unwrap();
    let full_input = driver.force_full_prepared_input_for_test(input).unwrap();
    assert_eq!(
        full_input
            .accounts
            .iter()
            .map(|rows| rows.len())
            .sum::<usize>(),
        9,
        "完整账户读取包含三组固定槽，再核真实名册/padding"
    );
    assert_eq!(
        full_input.personals.len(),
        6,
        "Full 含全部六个真实个人，不靠当前 dirty"
    );
    let full = game
        .prepare_runtime_mirror(&mut driver, full_input, 2)
        .unwrap();
    assert_eq!(
        normal.prepared_values_for_test(),
        full.prepared_values_for_test(),
        "同源两臂的所有安装值相等"
    );
    assert_eq!(driver.mirror_source(), source);
    assert_eq!(driver.world_committed_tick(), 2, "World 只执行一次第二拍");
    assert_eq!(driver.account_writes_for_test(), account_writes);
    assert_eq!(driver.personal_installs_for_test(), personal_writes);
    assert_eq!(
        serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
        before,
        "两臂 prepare 均不 swap"
    );
    let ack = game
        .validate_runtime_mirror_source(&driver, &normal)
        .unwrap();
    game.install_runtime_mirror(&mut driver, normal);
    driver.ack_prepared_mirror(ack);
    let installed_due: BTreeMap<_, _> = game
        .runtime_attention_state()
        .into_iter()
        .map(|(id, due, _)| (id, due))
        .collect();
    for id in &deferred {
        assert_eq!(installed_due[id], 99, "唯一 swap 必须安装真实推远调度值");
    }
}

/// 两份同静点 candidate 的 source 可以相同，但成功安装已经消费本次安装序号。
#[test]
fn second_candidate_from_same_static_point_is_rejected_after_first_swap() {
    let mut game = short_runtime_session();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    driver.adopt_session_seq(&game);
    let input = driver.prepare_driver_step(&game).unwrap();
    let first = game
        .prepare_runtime_mirror(&mut driver, input.clone(), 1)
        .unwrap();
    let second = game.prepare_runtime_mirror(&mut driver, input, 1).unwrap();
    let source = driver.mirror_source();
    let ack = game
        .validate_runtime_mirror_source(&driver, &first)
        .unwrap();
    game.install_runtime_mirror(&mut driver, first);
    driver.ack_prepared_mirror(ack);
    let installed = serde_json::to_value(game.snapshot_inner(true, true)).unwrap();
    let account_writes = driver.account_writes_for_test();
    let personal_writes = driver.personal_installs_for_test();
    let sequence = game.runtime_mirror_install_sequence;
    let domains = driver.pending_mirror_domains();
    assert_eq!(
        driver.mirror_source(),
        source,
        "安装本身不伪造 World 来源变化"
    );
    let rejected = game.validate_runtime_mirror_source(&driver, &second);
    assert!(
        rejected.is_err(),
        "第一次 swap 后，同源第二份 candidate 也必须按 install_sequence 拒绝"
    );
    assert_eq!(game.runtime_mirror_install_sequence, sequence);
    assert_eq!(
        driver.pending_mirror_domains(),
        domains,
        "拒绝不消费 pending 原因"
    );
    assert_eq!(
        driver.account_writes_for_test(),
        account_writes,
        "第二份 swap=0"
    );
    assert_eq!(driver.personal_installs_for_test(), personal_writes);
    assert_eq!(
        serde_json::to_value(game.snapshot_inner(true, true)).unwrap(),
        installed
    );
}

#[test]
fn full_read_preserves_nonempty_pending_and_real_tick_dirty_rows() {
    let mut game = short_runtime_session();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    let initial = driver.collect_personal_full().unwrap();
    let (local, id, mut row) = initial.groups[0]
        .1
        .iter()
        .find_map(|(local, id, row)| row.as_ref().map(|row| (*local, *id, row.clone())))
        .unwrap();
    row.attention.rng_state ^= 0x5137;
    driver.install_personal_for_test(local, row.clone());
    for _ in 0..2 {
        let full = driver.collect_personal_full().unwrap();
        assert_eq!(
            full.groups[0]
                .1
                .iter()
                .find(|(_, owner, _)| *owner == id)
                .unwrap()
                .2
                .as_ref(),
            Some(&row)
        );
    }
    let pending = driver.collect_group_belief_for_test().unwrap();
    assert!(
        pending[0]
            .1
            .iter()
            .any(|(slot, actual)| *slot == local && actual == &row),
        "Full 不能 take 非空 pending；首次 sparse 必须仍交付注入值"
    );
    let ids = driver.id_map.clone();
    driver.force_due_for_test_at(&ids, 1);
    game.runtime_step_with_install(&mut driver).unwrap();
    let published = driver.published_belief_shard_for_test(0);
    assert!(
        !published.is_empty(),
        "真实第一拍必须有个人演进，不能以空 dirty 自证"
    );
    let first = driver.collect_personal_full().unwrap();
    let second = driver.collect_personal_full().unwrap();
    assert_eq!(first.groups, second.groups);
    let dirty = driver.collect_group_belief_for_test().unwrap();
    assert_eq!(
        &dirty[0].1,
        published.as_ref(),
        "Full 不清真实 tick dirty，也不丢七字段"
    );
}

#[test]
fn day_end_policy_keeps_progress_without_installing_intraday_mirror() {
    let mut game = short_runtime_session();
    game.state.setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    let events = game.runtime_step_with_install(&mut driver).unwrap();
    assert_eq!(game.tick(), 1);
    assert_eq!(driver.world_committed_tick(), 1);
    assert_eq!(game.seq(), driver.next_seq());
    assert_eq!(
        driver.account_writes_for_test(),
        0,
        "DayEnd 日内不安装账户镜像"
    );
    assert_eq!(
        game.runtime_mirror_install_sequence, 0,
        "运行进度不能冒充镜像安装"
    );
    assert!(events.iter().all(|event| event.seq() <= game.seq()));
}

#[test]
fn day_end_policy_rejects_direct_ingress_before_receipt() {
    let mut game = short_runtime_session();
    game.state.setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let cursors = format!("{:?}", game.state.ingress_receipt_cursors);
    let intent = Intent::Cancel {
        code: StockId(0),
        id: OrderId(1),
    };
    assert!(matches!(
        game.enqueue_player_intent(AccountId(0), intent),
        Err(SessionError::SyncPolicyViolation(_))
    ));
    assert_eq!(format!("{:?}", game.state.ingress_receipt_cursors), cursors);
    assert!(game.state.pending_player.is_empty());
}

#[test]
fn day_end_policy_rejects_shared_ingress_before_receipt() {
    let mut game = short_runtime_session();
    game.state.setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let receiver = game.shared_ingress();
    let cursors = format!("{:?}", receiver.receipts_probe());
    let intent = Intent::Cancel {
        code: StockId(0),
        id: OrderId(1),
    };
    assert!(matches!(
        receiver.enqueue_player_intent(AccountId(0), intent),
        Err(SessionError::SyncPolicyViolation(_))
    ));
    assert_eq!(format!("{:?}", receiver.receipts_probe()), cursors);
}

#[test]
fn on_demand_explicit_flush_installs_without_advancing_progress() {
    let mut game = short_runtime_session();
    game.state.setup.runtime_sync_policy = crate::RuntimeSyncPolicy::OnDemand;
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    game.runtime_step_with_install(&mut driver).unwrap();
    assert_eq!(
        driver.account_writes_for_test(),
        0,
        "OnDemand 日内保持镜像原来源"
    );
    let progress = (game.tick(), game.seq(), driver.world_committed_tick());
    driver.flush_now(&mut game).unwrap();
    assert_eq!(
        (game.tick(), game.seq(), driver.world_committed_tick()),
        progress
    );
    assert!(driver.account_writes_for_test() > 0);
    assert_eq!(game.runtime_mirror_install_sequence, 1);
    driver.flush_now(&mut game).unwrap();
    assert_eq!(
        game.runtime_mirror_install_sequence, 1,
        "同来源重复 flush 是幂等静点读取"
    );
}

#[test]
fn current_save_requires_explicit_runtime_sync_policy() {
    let game = short_runtime_session();
    let mut value = serde_json::to_value(game.save().unwrap()).unwrap();
    value["setup"]
        .as_object_mut()
        .unwrap()
        .remove("runtime_sync_policy");
    let result = serde_json::from_value::<SaveSlot>(value);
    assert!(
        result.is_err(),
        "严格当前档不能用新局默认补齐缺少的同步策略"
    );
}

#[test]
fn delayed_preauction_progress_does_not_create_active_candle() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 12;
    setup.auction_ticks = 6;
    setup.closing_auction_ticks = 0;
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let mut game = GameSession::new(setup, 42).unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    let frame = game
        .runtime_step_with_prepared_mirror(&mut driver, |projection, facts| {
            projection.project_frame(facts)
        })
        .unwrap();
    assert!(
        frame.timeseries_payload.active_daily_candles.is_empty(),
        "开盘撮合前不能从昨价造活动 K"
    );
    assert!(!frame
        .events()
        .any(|event| matches!(event, Event::PriceTick { .. })));
    assert_eq!(game.runtime_mirror_origin().source_tick, 0);
    assert_eq!(game.tick(), 1);
}

#[test]
fn late_day_end_receiver_uses_current_progress_context() {
    let mut game = short_runtime_session();
    game.state.setup.runtime_sync_policy = crate::RuntimeSyncPolicy::DayEnd;
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    game.runtime_step_with_install(&mut driver).unwrap();
    game.runtime_step_with_install(&mut driver).unwrap();
    let receiver = game.shared_ingress();
    let cancel = Intent::Cancel {
        code: StockId(0),
        id: OrderId(1),
    };
    let SessionError::SyncPolicyViolation(direct) = game
        .enqueue_player_intent(AccountId(0), cancel.clone())
        .unwrap_err()
    else {
        panic!("必须为策略拒绝");
    };
    let SessionError::SyncPolicyViolation(shared) = receiver
        .enqueue_player_intent(AccountId(0), cancel)
        .unwrap_err()
    else {
        panic!("必须为策略拒绝");
    };
    assert_eq!(direct, shared);
    assert_eq!(direct.code, crate::error::RUNTIME_SYNC_POLICY_VIOLATION);
    assert_eq!(
        direct.context,
        vec![1, 2, driver.publication().load().generation]
    );
}
