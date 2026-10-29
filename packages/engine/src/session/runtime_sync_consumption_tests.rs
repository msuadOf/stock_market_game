use super::*;

/// 复用真实 runtime 报价夹具，并把交易日缩短到四拍以覆盖日界安装。
fn short_runtime_sync_session(policy: crate::RuntimeSyncPolicy) -> GameSession {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 4;
    setup.auction_ticks = 1;
    setup.closing_auction_ticks = 0;
    setup.runtime_sync_policy = policy;
    GameSession::new(setup, 42).unwrap()
}

fn assert_day_end_full_install(policy: crate::RuntimeSyncPolicy) {
    let mut game = short_runtime_sync_session(policy);
    game.fixture_account_mut(&AccountId(0))
        .unwrap()
        .fixture_set_positions(std::collections::BTreeMap::from([(
            StockId(0),
            crate::account::Position::from_restored_parts(100, 100, 100_000, 10_000),
        )]));

    for _ in 0..4 {
        game.step_frame().unwrap();
    }

    let origin = game.runtime_mirror_origin();
    assert_eq!(game.day(), 1, "真实末拍已推进交易日");
    assert_eq!(game.tick(), 4);
    assert_eq!(origin.source_tick, 4, "日界必须安装末拍权威账户");
    assert_eq!(
        origin.day_end_stage,
        crate::RuntimeMirrorDayEndStage::RetainedPublished,
        "完整日结镜像须晚于 retained 发布"
    );
    assert_eq!(
        game.account(AccountId(0)).unwrap().positions()[&StockId(0)].t1_locked(),
        0,
        "真实日结解冻非零 Player T+1 持仓"
    );
}

#[test]
fn on_demand_frames_without_pending_input_leave_mirror_at_baseline() {
    let mut game = short_runtime_sync_session(crate::RuntimeSyncPolicy::OnDemand);

    game.step_frame().unwrap();
    game.step_frame().unwrap();

    assert_eq!(game.tick(), 2, "两拍真实市场进度均已提交");
    assert_eq!(
        game.runtime_mirror_origin().source_tick,
        0,
        "没有待消费输入时 OnDemand 不逐拍安装账户镜像"
    );
}

#[test]
fn on_demand_player_input_flushes_before_next_frame_without_installing_that_frame() {
    let mut game = short_runtime_sync_session(crate::RuntimeSyncPolicy::OnDemand);
    game.step_frame().unwrap();
    let first_origin = game.runtime_mirror_origin();
    let ingress = game.shared_ingress();
    ingress
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: StockId(0),
                id: OrderId(1),
            },
        )
        .unwrap();

    game.step_frame().unwrap();

    let origin = game.runtime_mirror_origin();
    assert_eq!(game.tick(), 2, "第二拍市场进度已提交");
    assert_eq!(origin.source_tick, 1, "第二拍前的消费同步不安装第二拍账户");
    assert_eq!(
        origin.install_sequence,
        first_origin.install_sequence + 1,
        "消费同步成功后安装序号恰增一"
    );
}

#[test]
fn on_demand_flush_failure_keeps_world_and_accepted_ingress_and_seals_first_error() {
    let mut game = short_runtime_sync_session(crate::RuntimeSyncPolicy::OnDemand);
    game.step_frame().unwrap();
    let ingress = game.shared_ingress();
    ingress
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: StockId(0),
                id: OrderId(1),
            },
        )
        .unwrap();
    let accepted_receipts = ingress.receipts_probe();
    game.inject_runtime_install_failure();

    let first = game.step_frame().unwrap_err();

    assert_eq!(game.tick(), 1, "消费同步失败时不得执行第二拍");
    let driver = game.runtime_driver_for_test().unwrap().lock().unwrap();
    assert_eq!(driver.world_committed_tick(), 1, "已提交 World 保持原拍");
    drop(driver);
    assert_eq!(game.state.pending_player.len(), 1, "失败必须回灌已受理输入");
    assert!(matches!(
        game.state.pending_player.as_slice(),
        [ReceiptBearingIntent {
            owner: AccountId(0),
            intent: Intent::Cancel {
                code: StockId(0),
                id: OrderId(1)
            },
            account_ordinal: 0,
            stock_ordinal: 0,
        }]
    ));
    let after_failure_receipts = ingress.receipts_probe();
    assert_eq!(
        after_failure_receipts.next_account_ordinal,
        accepted_receipts.next_account_ordinal
    );
    assert_eq!(
        after_failure_receipts.next_stock_ordinal,
        accepted_receipts.next_stock_ordinal
    );
    assert_eq!(game.poison_reason(), Some(&first), "消费失败封存首个 fatal");
    assert!(matches!(
        ingress.enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: StockId(0),
                id: OrderId(2),
            }
        ),
        Err(SessionError::Step(actual)) if actual == first
    ));
}

#[test]
fn day_end_policy_installs_full_mirror_after_retained_publication() {
    assert_day_end_full_install(crate::RuntimeSyncPolicy::DayEnd);
}

#[test]
fn on_demand_policy_still_installs_full_mirror_after_retained_publication() {
    assert_day_end_full_install(crate::RuntimeSyncPolicy::OnDemand);
}

#[test]
fn delayed_first_buy_preserves_group_holding_entry_on_flush() {
    let mut setup = super::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    setup.start_date = crate::calendar::CivilDate::from_ymd(2026, 1, 5).unwrap();
    setup.ticks_per_day = 8;
    setup.runtime_sync_policy = crate::RuntimeSyncPolicy::OnDemand;
    let mut game = GameSession::new(setup, 42).unwrap();
    let buyer = AccountId(1);
    let seller = AccountId(2);
    let stock = StockId(0);
    game.fixture_account_mut(&seller)
        .unwrap()
        .fixture_set_positions(BTreeMap::from([(
            stock,
            crate::account::Position::from_restored_parts(1_000, 0, 1_000_000, 0),
        )]));
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    driver.defer_due_for_test_at(&[seller], u64::MAX);
    driver.force_due_for_test_at(&[buyer], 1);
    let receipts = [(seller, Side::Sell), (buyer, Side::Buy)]
        .into_iter()
        .enumerate()
        .map(|(ordinal, (owner, side))| ReceiptBearingIntent {
            owner,
            intent: Intent::PlaceLimit {
                code: stock,
                side,
                price: LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
            account_ordinal: 0,
            stock_ordinal: ordinal as u64,
        })
        .collect::<Vec<_>>();
    let upper = driver
        .world_public_views()
        .iter()
        .map(|view| view.daily_upper_limit)
        .collect::<Vec<_>>();
    driver.inject_player_intents(&receipts, &upper).unwrap();
    let events = game.runtime_step_with_install(&mut driver).unwrap();
    assert_eq!(events.iter().filter(|event| matches!(event, Event::Trade { maker, taker, qty: 100, .. } if *maker == seller && *taker == buyer)).count(), 1, "必须真实首买成交");
    assert!(
        game.account(buyer).unwrap().positions().is_empty(),
        "延迟镜像尚未看见首买"
    );
    driver.force_due_for_test_at(&[buyer], 2);
    game.runtime_step_with_install(&mut driver).unwrap();
    let full = driver.collect_personal_full().unwrap();
    let group_buyer = full
        .groups
        .iter()
        .flat_map(|(_, rows)| rows.iter())
        .find(|(_, id, _)| *id == buyer)
        .unwrap()
        .2
        .as_ref()
        .unwrap();
    let group_experience = group_buyer.retail_experience.as_ref().unwrap();
    assert!(
        group_experience.feedback.stocks.contains_key(&stock),
        "下一拍真实 due 观察已经创建持仓经历"
    );
    let expected = format!("{:?}", group_experience.feedback.stocks[&stock]);
    driver
        .flush_now(&mut game)
        .expect("flush 必须保已有组权威经历，不得重复首买初始化");
    assert_eq!(game.account(buyer).unwrap().positions()[&stock].qty(), 100);
    assert_eq!(
        format!(
            "{:?}",
            game.state.retail_experience[&buyer].feedback.stocks[&stock]
        ),
        expected
    );
    assert_eq!(
        (game.tick(), game.runtime_mirror_origin().source_tick),
        (2, 2)
    );
}

#[test]
fn initial_flush_preserves_validated_market_seed_before_first_publication() {
    let mut game = short_runtime_sync_session(crate::RuntimeSyncPolicy::OnDemand);
    let stock = StockId(0);
    game.state
        .markets
        .get_mut(&stock)
        .unwrap()
        .restore_prices(Money::from_cents(1_100), Money::from_cents(1_000));
    game.state.price_history.insert(
        stock,
        VecDeque::from([Money::from_cents(1_000), Money::from_cents(1_100)]),
    );
    let price_history = game.state.price_history.clone();
    let active = game.state.candle_book.active().clone();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    driver.flush_now(&mut game).unwrap();
    assert_eq!(
        game.state.markets[&stock].last_price(),
        Money::from_cents(1_100),
        "v0占位不得覆盖真实初始价格"
    );
    assert_eq!(
        game.state.price_history, price_history,
        "初始静点保留真实历史"
    );
    assert_eq!(
        game.state.candle_book.active(),
        &active,
        "初始静点不得补造活动 K"
    );
    assert_eq!((game.tick(), game.seq()), (0, 0));
}
