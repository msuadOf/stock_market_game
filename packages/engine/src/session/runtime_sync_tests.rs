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
    assert_eq!(game.save().unwrap_err(), fatal);
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
    assert_eq!(game.save().unwrap_err(), fatal);
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
    assert_eq!(game.save().unwrap_err(), fatal);
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
