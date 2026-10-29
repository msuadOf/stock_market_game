//! D2b 默认 Retail 生命周期的真实短 fixture；本文件不接通 feature rich 来源。

use super::*;
use crate::runtime::drive::RuntimeDriver;

const SELLER: AccountId = AccountId(1);
const BUYER: AccountId = AccountId(2);
const CODE: StockId = StockId(0);

fn assert_snapshot_matches_except_static_mirror_generation(
    before: &serde_json::Value,
    after: &serde_json::Value,
) {
    let mut expected = before.clone();
    let expected_origin = expected["mirror_origin"].as_object_mut().unwrap();
    for field in ["authority_epoch", "install_sequence"] {
        let value = expected_origin[field].as_u64().unwrap();
        expected_origin.insert(field.to_owned(), serde_json::json!(value + 1));
    }
    assert_eq!(*after, expected, "静点接管仅推进镜像来源代数");
}

fn fixture(ticks_per_day: u64) -> (GameSession, RuntimeDriver) {
    let mut setup = super::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.stocks[0].float_shares = 2_000;
    setup.float_allocation =
        FloatAllocation::class_percentages(1.0, 0.0, 0.0, WithinKindDistribution::EqualPercentage);
    setup.ticks_per_day = ticks_per_day;
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.config.price_cage_enabled = true;
    let mut game = GameSession::new(setup, 21).expect("构造双 Retail 会话");
    for owner in [SELLER, BUYER] {
        let account = game.account(owner).expect("Retail 必须在册");
        assert_eq!(account.kind(), AccountKind::Retail);
        // 保留构造器同源的现金、旧仓与 experience，不人工制造风险回撤。
        assert_eq!(account.sellable_qty(&CODE), 1_000);
    }
    let mut driver = RuntimeDriver::from_session(&mut game).expect("从真实名册构造 driver");
    driver.adopt_session_seq(&game);
    let upper = driver.world_public_views()[CODE.0 as usize].daily_upper_limit;
    let required = crate::runtime::fees::buy_purse_limit(
        &game.state.setup.config,
        Money::from_cents(upper),
        300,
        Money::ZERO,
    )
    .expect("按真实涨停参考价计算 300 股最坏买单金额及费用");
    assert!(
        game.account(BUYER).expect("真实买腿账户").cash() >= required,
        "构造期现金必须足够承担 300 股按涨停价计算的最坏 purse，含真实佣金和过户费"
    );
    // 到期指令只决定 observe 时机；DecisionTrace 必须由真实观察路径生成。
    driver.force_due_for_test_at(&[SELLER, BUYER], 1);
    (game, driver)
}

fn limit(side: Side, price: i64, qty: u32) -> Intent {
    Intent::PlaceLimit {
        code: CODE,
        side,
        price: LimitPrice::Fixed(Money::from_cents(price)),
        qty,
    }
}

fn inject(driver: &mut RuntimeDriver, intents: Vec<(AccountId, Intent)>) {
    let upper = driver
        .world_public_views()
        .iter()
        .map(|view| view.daily_upper_limit)
        .collect::<Vec<_>>();
    let receipts = intents
        .into_iter()
        .enumerate()
        .map(|(ordinal, (owner, intent))| ReceiptBearingIntent {
            owner,
            intent,
            account_ordinal: ordinal as u64,
            stock_ordinal: ordinal as u64,
        })
        .collect::<Vec<_>>();
    // 既有名册外部路由支持 NPC owner；enqueue_player_intent 不能替代它。
    driver
        .inject_player_intents(&receipts, &upper)
        .expect("真实 Retail 意图路由");
}

fn resting_buy(events: &[Event], remaining: u32) -> OrderId {
    let ids = events
        .iter()
        .filter_map(|event| match event {
            Event::OrderAccepted {
                account,
                code,
                side: Side::Buy,
                id,
                remaining_qty,
                ..
            } if *account == BUYER && *code == CODE && *remaining_qty == remaining => Some(*id),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ids.len(), 1, "必须有且仅有一笔指定余量的真实买单");
    ids[0]
}

fn assert_trade_legs(game: &GameSession, driver: &RuntimeDriver, events: &[Event], qty: u32) {
    let record = driver.world_report_log().last().expect("真实 TickRecord");
    assert_eq!(record.tick, game.tick());
    assert_eq!(record.desk_trades.len(), 1, "不能用零成交 fixture 通过");
    let trade = record.desk_trades[0];
    assert_eq!(u32::from(trade.stock), CODE.0);
    assert_eq!(trade.qty, qty);
    assert_eq!(driver.account_by_owner(trade.maker), Some(SELLER));
    assert_eq!(driver.account_by_owner(trade.taker), Some(BUYER));
    let public = events
        .iter()
        .filter(|event| {
            matches!(event,
                Event::Trade { code, maker, taker, qty: actual, price, .. }
                    if *code == CODE && *maker == SELLER && *taker == BUYER
                        && *actual == qty && price.cents() == trade.price
            )
        })
        .count();
    assert_eq!(public, 1, "公开 Trade 必须和真实双腿事实一致");
    let mut actual = game
        .last_retail_order_events()
        .iter()
        .filter_map(|event| match event {
            RetailOrderDiagnosticEvent::Filled {
                account,
                code,
                side,
                order_id,
                qty,
            } => Some((account.0, code.0, *side as u8, order_id.0, *qty)),
            _ => None,
        })
        .collect::<Vec<_>>();
    let mut expected = vec![
        (
            SELLER.0,
            CODE.0,
            Side::Sell as u8,
            trade.maker_order_serial,
            qty,
        ),
        (
            BUYER.0,
            CODE.0,
            Side::Buy as u8,
            trade.taker_order_serial,
            qty,
        ),
    ];
    actual.sort_unstable();
    expected.sort_unstable();
    assert_eq!(
        actual, expected,
        "每一 Filled 腿必须按真实 Trade 的 owner/ID/qty 对账"
    );
}

fn partial_fill(game: &mut GameSession, driver: &mut RuntimeDriver) -> OrderId {
    inject(
        driver,
        vec![
            (SELLER, limit(Side::Sell, 1_000, 100)),
            (BUYER, limit(Side::Buy, 1_000, 300)),
        ],
    );
    let events = game
        .runtime_step_with_install(driver)
        .expect("同拍真实部分成交");
    let id = resting_buy(&events, 200);
    assert_trade_legs(game, driver, &events, 100);
    assert_eq!(
        game.last_retail_order_events().len(),
        4,
        "双 Retail 各一 Submitted 和一 Filled，不能重复或遗漏"
    );
    let seller_id = OrderId(
        driver
            .world_report_log()
            .last()
            .expect("真实成交记录")
            .desk_trades[0]
            .maker_order_serial,
    );
    assert!(game
        .last_retail_order_events()
        .contains(&RetailOrderDiagnosticEvent::Submitted {
            account: SELLER,
            code: CODE,
            side: Side::Sell,
            order_id: seller_id,
            qty: 100,
        }));
    assert!(
        game.last_retail_order_events()
            .contains(&RetailOrderDiagnosticEvent::Submitted {
                account: BUYER,
                code: CODE,
                side: Side::Buy,
                order_id: id,
                qty: 300,
            }),
        "Submitted 必须保留原 300 股，不能用 OrderAccepted 的余 200 股代替"
    );
    assert_eq!(
        game.last_retail_order_events()
            .iter()
            .filter(|event| matches!(event,
                RetailOrderDiagnosticEvent::Submitted { account, .. } if *account == BUYER
            ))
            .count(),
        1
    );
    assert!(
        !game.last_retail_decisions().is_empty(),
        "真实 observe 必须生成非空 prior decisions"
    );
    // 后续拍不再触发随机 observe；只隔离本 fixture 的业务输入。
    driver.defer_due_for_test_at(&[SELLER, BUYER], 100);
    id
}

#[test]
fn d2b_retail_same_tick_300_fills_100_and_cancels_200() {
    let (mut game, mut driver) = fixture(100);
    let cost = crate::runtime::retail_lifecycle::cost_probe::Arm::new();
    driver
        .collect_resting_facts_live()
        .expect("读取真实初始 desk 游标");
    let first = driver.desk_next_serials()[0];
    // 两次 Place 后才处理同批 Cancel；预测仅用于命令，结果再对真实 Trade 身份。
    let requested = OrderId(first.checked_add(1).expect("短 fixture 序号无溢出"));
    inject(
        &mut driver,
        vec![
            (SELLER, limit(Side::Sell, 1_000, 100)),
            (BUYER, limit(Side::Buy, 1_000, 300)),
            (
                BUYER,
                Intent::Cancel {
                    code: CODE,
                    id: requested,
                },
            ),
        ],
    );
    let events = game
        .runtime_step_with_install(&mut driver)
        .expect("同拍下单、成交、撤余量");
    let actual = resting_buy(&events, 200);
    assert_eq!(actual, requested);
    assert_trade_legs(&game, &driver, &events, 100);
    let diagnostics = game.last_retail_order_events();
    assert_eq!(diagnostics.len(), 5);
    let seller_id = OrderId(
        driver
            .world_report_log()
            .last()
            .expect("真实卖腿记录")
            .desk_trades[0]
            .maker_order_serial,
    );
    assert!(
        diagnostics.contains(&RetailOrderDiagnosticEvent::Submitted {
            account: SELLER,
            code: CODE,
            side: Side::Sell,
            order_id: seller_id,
            qty: 100,
        }),
        "剩余一条诊断必须是实际卖腿原 100 股 Submitted"
    );
    let submitted = diagnostics
        .iter()
        .position(|event| {
            *event
                == RetailOrderDiagnosticEvent::Submitted {
                    account: BUYER,
                    code: CODE,
                    side: Side::Buy,
                    order_id: actual,
                    qty: 300,
                }
        })
        .expect("真实原 300 股 Submitted");
    let filled = diagnostics
        .iter()
        .position(|event| {
            *event
                == RetailOrderDiagnosticEvent::Filled {
                    account: BUYER,
                    code: CODE,
                    side: Side::Buy,
                    order_id: actual,
                    qty: 100,
                }
        })
        .expect("真实 100 股 Filled");
    let canceled = diagnostics
        .iter()
        .position(|event| {
            *event
                == RetailOrderDiagnosticEvent::Canceled {
                    account: BUYER,
                    code: CODE,
                    order_id: actual,
                    remaining_qty: 200,
                }
        })
        .expect("真实剩余 200 股 Canceled");
    assert!(
        submitted < filled && filled < canceled,
        "同单真实操作序不能从聚合后的事件数组倒推"
    );
    assert!(events.iter().any(|event| matches!(event,
        Event::OrderCanceled { account, code, id, remaining_qty: 200, .. }
            if *account == BUYER && *code == CODE && *id == actual
    )));
    assert!(driver.collect_resting_facts().expect("同拍撤后真实订单簿")[0].is_empty());
    let reports = cost.take();
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert_eq!((report.raw_len, report.resolved_len), (4, 5));
    assert!(report.raw_capacity <= 8 && report.resolved_capacity <= 8);
    assert_eq!(
        (report.raw_element_bytes, report.resolved_element_bytes),
        (32, 56)
    );
    assert!(driver
        .world_report_log()
        .iter()
        .all(|record| record.retail.is_empty()));
    println!("D2b default 300/100/200 report cost: {reports:?}");
}

#[test]
fn d2b_retail_original_300_fills_100_then_real_cancel_200() {
    let (mut game, mut driver) = fixture(100);
    let id = partial_fill(&mut game, &mut driver);
    inject(
        &mut driver,
        vec![(BUYER, Intent::Cancel { code: CODE, id })],
    );
    let events = game
        .runtime_step_with_install(&mut driver)
        .expect("真实撤销部分成交余单");
    assert_eq!(
        game.last_retail_order_events(),
        &[RetailOrderDiagnosticEvent::Canceled {
            account: BUYER,
            code: CODE,
            order_id: id,
            remaining_qty: 200
        },]
    );
    assert!(events.iter().any(|event| matches!(event,
        Event::OrderCanceled { account, code, id: actual, remaining_qty: 200, .. }
            if *account == BUYER && *code == CODE && *actual == id
    )));
    assert!(driver.collect_resting_facts().expect("撤后真实订单簿")[0].is_empty());
    assert!(
        game.last_retail_decisions().is_empty(),
        "成功空 observe 批次须清空 prior decisions"
    );
}

#[test]
fn d2b_retail_early_round_and_next_tick_fill_keep_real_order_id() {
    let (mut game, mut driver) = fixture(100);
    driver.force_continuation_for_test();
    let id = partial_fill(&mut game, &mut driver);
    assert_eq!(
        driver
            .world_report_log()
            .last()
            .expect("多轮真实记录")
            .rounds,
        1,
        "轮 0 + 轮 1，rounds 保存最后轮号 1"
    );
    inject(&mut driver, vec![(SELLER, limit(Side::Sell, 1_000, 100))]);
    let events = game
        .runtime_step_with_install(&mut driver)
        .expect("跨 tick 同单继续成交");
    assert_trade_legs(&game, &driver, &events, 100);
    assert!(game
        .last_retail_order_events()
        .contains(&RetailOrderDiagnosticEvent::Filled {
            account: BUYER,
            code: CODE,
            side: Side::Buy,
            order_id: id,
            qty: 100,
        }));
    assert!(
        !game
            .last_retail_order_events()
            .iter()
            .any(|event| matches!(event,
                RetailOrderDiagnosticEvent::Submitted { order_id, .. } if *order_id == id
            )),
        "旧活动单不得补造本拍 Submitted"
    );
    let resting = driver
        .collect_resting_facts()
        .expect("续成交后的真实订单簿");
    assert_eq!(resting[0].len(), 1);
    assert_eq!(resting[0][0].order_serial, id.0);
    assert_eq!(resting[0][0].qty, 100);
}

#[test]
fn d2b_retail_desk_reject_consumes_id_and_closes_original_attempt() {
    let (mut game, mut driver) = fixture(100);
    driver
        .collect_resting_facts_live()
        .expect("拒单前真实 desk 游标");
    let first = driver.desk_next_serials()[0];
    // 1050 分在 900..1100 涨跌幅带内，超过空簿初始买入价格笼子。
    inject(&mut driver, vec![(BUYER, limit(Side::Buy, 1_050, 300))]);
    let events = game
        .runtime_step_with_install(&mut driver)
        .expect("default-off 真实 desk 业务拒单");
    let record = driver.world_report_log().last().expect("拒单真实记录");
    assert!(record.group_rejections.is_empty());
    assert_eq!(record.desk_receipts.len(), 1);
    let receipt = &record.desk_receipts[0];
    assert_eq!(receipt.kind, crate::runtime::ReceiptKind::PriceCageExceeded);
    assert_eq!(driver.account_by_owner(receipt.owner), Some(BUYER));
    let id = OrderId(receipt.order.1);
    assert_eq!(id.0, first);
    assert!(id.0 > 0, "desk 真实 ID 不能是 group sentinel 0");
    assert_eq!(
        driver.desk_next_serials()[0],
        id.0 + 1,
        "真实拒单必须消费一个 desk ID"
    );
    assert!(events.iter().any(|event| matches!(event,
        Event::IntentRejected { account, code, reason: RejectionReason::PriceCageExceeded, .. }
            if *account == BUYER && *code == CODE
    )));
    assert_eq!(
        game.last_retail_order_events(),
        &[
            RetailOrderDiagnosticEvent::Submitted {
                account: BUYER,
                code: CODE,
                side: Side::Buy,
                order_id: id,
                qty: 300
            },
            RetailOrderDiagnosticEvent::Aborted {
                account: BUYER,
                code: CODE,
                order_id: id,
                remaining_qty: 300
            },
            RetailOrderDiagnosticEvent::Rejected {
                account: BUYER,
                code: CODE,
                reason: RejectionReason::PriceCageExceeded
            },
        ]
    );
    assert!(record.desk_trades.is_empty());
    assert!(driver.collect_resting_facts().expect("拒单无在途单")[0].is_empty());
}

fn group_reject_prior(game: &mut GameSession, driver: &mut RuntimeDriver) {
    driver
        .collect_resting_facts_live()
        .expect("组拒单前真实 desk 游标");
    let first = driver.desk_next_serials()[0];
    // 旧可卖仓 1000 股时卖 101 股不是一次性清零零股，组预检直接拒绝。
    inject(driver, vec![(SELLER, limit(Side::Sell, 1_000, 101))]);
    game.runtime_step_with_install(driver)
        .expect("真实 group 预检拒单");
    let record = driver.world_report_log().last().expect("组拒单真实记录");
    assert!(record.desk_receipts.is_empty());
    assert_eq!(record.group_rejections.len(), 1);
    assert_eq!(record.group_rejections[0].order, (CODE.0 as u16, 0));
    assert_eq!(
        driver.desk_next_serials()[0],
        first,
        "group 预检不得消费 desk ID"
    );
    assert_eq!(
        game.last_retail_order_events(),
        &[RetailOrderDiagnosticEvent::Rejected {
            account: SELLER,
            code: CODE,
            reason: RejectionReason::InvalidQuantity
        },]
    );
    assert!(
        !game.last_retail_decisions().is_empty(),
        "非空 decisions 必须来自真实 observe"
    );
}

#[test]
fn d2b_retail_group_precheck_has_only_rejected_and_no_desk_id() {
    let (mut game, mut driver) = fixture(100);
    group_reject_prior(&mut game, &mut driver);
}

#[test]
fn d2b_plain_late_day_end_failure_preserves_nonempty_prior_with_expected_poison() {
    let (mut game, mut driver) = fixture(2);
    group_reject_prior(&mut game, &mut driver);
    let prior_events = game.last_retail_order_events().to_vec();
    let prior_decisions = game.last_retail_decisions().to_vec();
    let before_business = game.business_state_hash().expect("失败前业务 hash");
    let mut oracle = game
        .clone_for_tick_shadow()
        .expect("健康 checkpoint oracle");
    let validator_hit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let hit = std::sync::Arc::clone(&validator_hit);
    driver.set_day_end_validator_for_test(Box::new(move |_| {
        hit.store(true, std::sync::atomic::Ordering::SeqCst);
        Err("D2b 日结晚失败".to_owned())
    }));
    let fatal = game
        .runtime_step_with_install(&mut driver)
        .expect_err("capture_undo=false 日结晚失败");
    assert!(
        validator_hit.load(std::sync::atomic::Ordering::SeqCst),
        "必须真实到达日结 validator：{fatal:?}"
    );
    // cd6 后日结属于统一私有 prepare；仍精确核 kind、当前来源及原失败 marker。
    assert!(
        matches!(&fatal, StepFatal::InvariantViolation { location, description }
        if location == "runtime_mirror::prepare" && description.contains("日结私有准备失败") && description.contains("D2b 日结晚失败")),
        "{fatal:?}"
    );
    assert_eq!(game.poison_reason(), Some(&fatal));
    assert_eq!(game.last_retail_order_events(), prior_events.as_slice());
    assert_eq!(game.last_retail_decisions(), prior_decisions.as_slice());
    assert_eq!(
        game.business_state_hash().expect("失败后业务 hash"),
        before_business
    );
    // 普通失败只允许已有 poison 元数据变化，不移除 poison 来凑 hash。
    oracle.poison = Some(fatal);
    assert_eq!(
        game.session_state_hash().expect("失败后完整 hash"),
        oracle
            .session_state_hash()
            .expect("仅 expected poison 的完整 hash")
    );
}

#[test]
fn d2b_internal_late_callback_failure_preserves_prior_with_expected_poison() {
    let (mut game, mut driver) = fixture(100);
    group_reject_prior(&mut game, &mut driver);
    let prior_events = game.last_retail_order_events().to_vec();
    let prior_decisions = game.last_retail_decisions().to_vec();
    game.runtime_driver = Some(std::sync::Mutex::new(driver));
    let before_business = game
        .business_state_hash()
        .expect("callback 前完整业务 hash");
    let mut oracle = game
        .clone_for_tick_shadow()
        .expect("callback 前健康 checkpoint");
    let fatal = StepFatal::InvariantViolation {
        location: "d2b_internal_callback".to_owned(),
        description: "真实安装后拒绝候选".to_owned(),
    };
    let mut called = false;
    let error = game
        .step_frame_and_commit_via_runtime(|_, _| {
            called = true;
            Err::<(), _>(fatal.clone())
        })
        .expect_err("capture_undo=true 内部 callback 必须失败");
    assert!(called, "必须到达安装后的内部 callback");
    assert_eq!(error, fatal);
    // 新版本采用 E2 DP1：World 发布后的 ordinary 晚失败必须 terminal。
    // source9 原七例的无 poison 旧断言及首次结果永久保留于 07ee516e。
    assert_eq!(game.poison_reason(), Some(&fatal));
    oracle.poison_runtime_step(fatal.clone());
    assert_eq!(
        game.business_state_hash()
            .expect("callback 失败后业务 hash"),
        before_business
    );
    assert_eq!(game.last_retail_order_events(), prior_events.as_slice());
    assert_eq!(game.last_retail_decisions(), prior_decisions.as_slice());
    assert_eq!(
        game.session_state_hash().expect("callback 失败后完整 hash"),
        oracle
            .session_state_hash()
            .expect("仅 expected poison 的完整 hash")
    );
    assert!(
        game.runtime_driver.is_some(),
        "E2 terminal driver 保留首错与已发布 raw 事实"
    );
    let world_tick = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .world_committed_tick();
    assert_eq!(game.step_via_runtime().unwrap_err(), fatal);
    assert_eq!(
        game.runtime_driver
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .world_committed_tick(),
        world_tick,
        "terminal 后健康门必须拒绝再次推进 World"
    );
    assert!(matches!(game.save(), Err(crate::SessionError::Step(actual)) if actual == fatal));
}

#[test]
fn d2b_install_failure_preserves_exact_prior_vectors_and_expected_poison_hash() {
    let (mut game, mut driver) = fixture(100);
    group_reject_prior(&mut game, &mut driver);
    let prior_events = game.last_retail_order_events().to_vec();
    let prior_decisions = game.last_retail_decisions().to_vec();
    let before_business = game.business_state_hash().unwrap();
    let mut oracle = game.clone_for_tick_shadow().unwrap();
    let world_before = driver.world_committed_tick();
    let account_writes = driver.account_writes_for_test();
    let personal_writes = driver.personal_installs_for_test();
    game.inject_runtime_install_failure();
    let error = game.runtime_step_with_install(&mut driver).unwrap_err();
    assert!(
        matches!(&error, StepFatal::InvariantViolation { location, description }
        if location == "runtime_mirror::prepare" && description.contains("安装失败注入"))
    );
    assert_eq!(
        driver.world_committed_tick(),
        world_before + 1,
        "World 已发布，Session candidate 未安装"
    );
    assert_eq!(driver.account_writes_for_test(), account_writes);
    assert_eq!(driver.personal_installs_for_test(), personal_writes);
    assert_eq!(game.last_retail_order_events(), prior_events);
    assert_eq!(game.last_retail_decisions(), prior_decisions);
    assert_eq!(game.business_state_hash().unwrap(), before_business);
    oracle.poison_runtime_step(error);
    assert_eq!(
        game.session_state_hash().unwrap(),
        oracle.session_state_hash().unwrap()
    );
}

#[test]
fn d2b_manual_outer_failure_never_publishes_or_clears_prior_batch() {
    for outer in 0..3 {
        let (mut game, mut driver) = fixture(100);
        partial_fill(&mut game, &mut driver);
        let prior_events = game.last_retail_order_events().to_vec();
        let prior_decisions = game.last_retail_decisions().to_vec();
        driver.set_day_end_validator_for_test(Box::new(|_| Err("D2b manual Drain拒绝".to_owned())));
        let error = match outer {
            0 => driver.day_end(&mut game).unwrap_err(),
            1 => driver
                .day_end_with_runtime_market_day(&mut game)
                .unwrap_err(),
            _ => driver.day_end_with_market(&mut game).unwrap_err(),
        };
        assert!(error.contains("D2b manual Drain拒绝"));
        assert_eq!(game.last_retail_order_events(), prior_events);
        assert_eq!(game.last_retail_decisions(), prior_decisions);
        assert_eq!(driver.collect_resting_facts().unwrap()[0].len(), 1);
    }
}

#[test]
fn d2b_successful_empty_tick_clears_both_vectors_and_report_does_not_retain_copy() {
    let (mut game, mut driver) = fixture(100);
    group_reject_prior(&mut game, &mut driver);
    driver.defer_due_for_test_at(&[SELLER, BUYER], 100);
    game.runtime_step_with_install(&mut driver).unwrap();
    assert!(game.last_retail_order_events().is_empty());
    assert!(game.last_retail_decisions().is_empty());
    assert!(driver
        .world_report_log()
        .iter()
        .all(|record| record.retail.is_empty()));
}

#[test]
fn d2b_each_successful_manual_outer_publishes_only_drain_and_clears_decisions() {
    for outer in 0..3 {
        let (mut game, mut driver) = fixture(2);
        let order_id = partial_fill(&mut game, &mut driver);
        driver.step_frame_equivalent(&mut game).unwrap();
        driver.install_market_views(&mut game).unwrap();
        driver.install_candles(&mut game).unwrap();
        assert_eq!(game.tick(), 2);
        if outer != 1 {
            game.advance_runtime_market_day();
        }
        match outer {
            0 => driver.day_end(&mut game).unwrap(),
            1 => driver.day_end_with_runtime_market_day(&mut game).unwrap(),
            _ => driver.day_end_with_market(&mut game).unwrap(),
        }
        assert_eq!(
            game.last_retail_order_events(),
            &[RetailOrderDiagnosticEvent::Canceled {
                account: BUYER,
                code: CODE,
                order_id,
                remaining_qty: 200
            },]
        );
        assert!(
            game.last_retail_decisions().is_empty(),
            "manual Drain 没有新的 observe，不能保留旧 decisions"
        );
        // 同 tick Drain 后 PublishedTick 仍是清簿前快照；空簿断言须查真实 desk。
        assert!(driver.collect_resting_facts_live().unwrap()[0].is_empty());
    }
}

#[test]
fn prepared_retail_lifecycle_is_repeatable_and_failure_preserves_pending() {
    let (mut game, mut driver) = fixture(100);
    inject(
        &mut driver,
        vec![
            (SELLER, limit(Side::Sell, 1_000, 100)),
            (BUYER, limit(Side::Buy, 1_000, 300)),
        ],
    );
    let input = driver.prepare_driver_step(&game).unwrap();
    assert_eq!(
        driver.world_report_log().last().unwrap().desk_trades.len(),
        1,
        "必须有真实部分成交，不能用空 pending 自证"
    );
    let first = game
        .prepare_runtime_mirror(&mut driver, input.clone(), 1)
        .unwrap();
    assert_eq!(
        first.retail_order_events_for_test().len(),
        4,
        "两 Retail 各有一 Submitted 和一 Filled"
    );
    let second = game
        .prepare_runtime_mirror(&mut driver, input.clone(), 1)
        .unwrap();
    assert_eq!(
        second.retail_order_events_for_test(),
        first.retail_order_events_for_test(),
        "同一 World 静点的完整 candidate 不得消费待安装 Retail 生命周期事实"
    );
    game.runtime_install_failure = true;
    assert!(game
        .prepare_runtime_mirror(&mut driver, input.clone(), 1)
        .is_err());
    game.runtime_install_failure = false;
    let after_failure = game.prepare_runtime_mirror(&mut driver, input, 1).unwrap();
    assert_eq!(
        after_failure.retail_order_events_for_test(),
        first.retail_order_events_for_test(),
        "失败 prepare 必须保留全部待安装 Retail 生命周期事实"
    );
    let ack = game
        .validate_runtime_mirror_source(&driver, &after_failure)
        .unwrap();
    game.install_runtime_mirror(&mut driver, after_failure);
    driver.ack_prepared_mirror(ack);
    assert_eq!(
        game.last_retail_order_events(),
        first.retail_order_events_for_test()
    );
}

fn legacy_closing_price_fixture(first_order_id: u64) -> GameSession {
    let mut setup = super::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.stocks[0].float_shares = 2_000;
    setup.float_allocation =
        FloatAllocation::class_percentages(1.0, 0.0, 0.0, WithinKindDistribution::EqualPercentage);
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.ticks_per_day = 8;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 2;
    let mut game = GameSession::new(setup, 21).unwrap();
    let mut seed_events = Vec::new();
    game.state.next_order_id = first_order_id;
    // Player 老单没有 NPC quote expiry，必须真实留到 Closing。
    game.seed_order_for_test(AccountId(0), limit(Side::Buy, 990, 100), &mut seed_events);
    game.seed_order_for_test(SELLER, limit(Side::Sell, 995, 100), &mut seed_events);
    game.enqueue_player_intent(AccountId(0), limit(Side::Buy, 995, 100))
        .unwrap();
    let traded = game.step_pipeline_contract().unwrap();
    assert!(
        traded.iter().any(
            |event| matches!(event, Event::Trade { price, qty: 100, .. } if price.cents() == 995)
        ),
        "必须经真实撮合把最新价从开局 1000 改成 995"
    );
    for _ in 0..6 {
        if game.phase() == TradingPhase::ClosingAuction {
            break;
        }
        game.step_pipeline_contract().unwrap();
    }
    assert_eq!(game.phase(), TradingPhase::ClosingAuction);
    assert_eq!(game.state.markets[&CODE].last_price().cents(), 995);
    let cutoff = game.runtime_public_cutoff_for(&CODE);
    assert!(cutoff > 0, "必须有真实收盘竞价公开截点，不能用零值自证");
    let old = game.state.markets[&CODE]
        .resting_orders()
        .into_iter()
        .find(|order| order.id == OrderId(first_order_id))
        .expect("Player 老买单必须留在真实旧簿");
    assert_eq!(old.owner, AccountId(0));
    assert_eq!(old.qty, 100);
    assert!(old.seq < cutoff, "真实老单在旧公开截点之前");
    game
}

#[test]
fn lazy_static_handover_preserves_real_trade_price_and_closing_cutoff() {
    let game = legacy_closing_price_fixture(1);
    let cutoff = game.runtime_public_cutoff_for(&CODE);
    let before = game.save().unwrap();
    assert!(!before.desk_mirrored_resting);
    let mut restored = GameSession::restore(&before).unwrap();
    let snapshot = serde_json::to_value(restored.snapshot_inner(true, true)).unwrap();
    restored.construct_runtime_driver_lazy_for_test().unwrap();
    let handed_over = serde_json::to_value(restored.snapshot_inner(true, true)).unwrap();
    assert_snapshot_matches_except_static_mirror_generation(&snapshot, &handed_over);
    {
        let driver = restored.runtime_driver.as_ref().unwrap().lock().unwrap();
        assert_ne!(
            driver.world_public_views()[0].last_price,
            995,
            "必须判别旧 v0 价格来源错误"
        );
        assert_ne!(
            driver.world_public_views()[0].public_cutoff_serial,
            cutoff,
            "必须判别旧 v0 截点来源错误"
        );
    }
    assert_eq!(
        restored.runtime_public_cutoff_for(&CODE),
        cutoff,
        "首次静点镜像不得把真实收盘截点降回 v0 的零值"
    );
    assert_eq!(
        restored.state.desk_book_mirror[&CODE].last_price.cents(),
        995,
        "首次静点镜像不得把真实成交价降回开局价"
    );
    assert_eq!(
        serde_json::to_value(restored.snapshot_inner(true, true)).unwrap(),
        handed_over
    );
    GameSession::restore(&restored.save().unwrap()).unwrap();
}

#[test]
fn lazy_handover_rejects_changed_closing_visibility_without_mutation() {
    let game = legacy_closing_price_fixture(7);
    let cutoff = game.runtime_public_cutoff_for(&CODE);
    let old = game.state.markets[&CODE]
        .resting_orders()
        .into_iter()
        .find(|order| order.id == OrderId(7))
        .expect("拒绝前必须有真实在场 id7");
    assert_ne!(
        old.seq < cutoff,
        crate::runtime::group::desk_order_serial(old.id.0) < cutoff,
        "负控必须真实覆盖旧 seq 与 desk serial 的公开分类不同"
    );
    let saved = game.save().unwrap();
    let mut restored = GameSession::restore(&saved).expect("真实 closing 旧簿存档必须合法");
    let before = serde_json::to_value(&saved).unwrap();
    let revision = restored.external_revision();
    let epoch = restored.runtime_authority_epoch;
    let install = restored.runtime_mirror_install_sequence;
    let error = restored
        .construct_runtime_driver_lazy_for_test()
        .unwrap_err();
    assert!(error.to_string().contains("公开截点"));
    assert!(restored.require_healthy().is_ok());
    assert!(restored.runtime_driver.is_none());
    assert_eq!(restored.external_revision(), revision);
    assert_eq!(restored.runtime_authority_epoch, epoch);
    assert_eq!(restored.runtime_mirror_install_sequence, install);
    assert_eq!(
        serde_json::to_value(restored.save().unwrap()).unwrap(),
        before
    );
}

#[test]
fn lazy_handover_preserves_same_price_fifo_before_and_after_save_restore() {
    let mut setup = super::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 3;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.stocks[0].float_shares = 3_000;
    setup.float_allocation =
        FloatAllocation::class_percentages(1.0, 0.0, 0.0, WithinKindDistribution::EqualPercentage);
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    let mut source = GameSession::new(setup.clone(), 21).unwrap();
    source.state.next_order_id = 7;
    let mut seed_events = Vec::new();
    // 先到 owner2、后到 owner1，反向于 ledger 的 owner 键序。
    source.seed_order_for_test(BUYER, limit(Side::Buy, 990, 100), &mut seed_events);
    source.seed_order_for_test(SELLER, limit(Side::Buy, 990, 100), &mut seed_events);
    let mut reversed = GameSession::new(setup, 21).unwrap();
    reversed.state.next_order_id = 9;
    reversed.seed_order_for_test(BUYER, limit(Side::Buy, 990, 100), &mut seed_events);
    reversed.state.next_order_id = 7;
    reversed.seed_order_for_test(SELLER, limit(Side::Buy, 990, 100), &mut seed_events);
    reversed.state.next_order_id = 10;
    let invalid = reversed.save().unwrap();
    let mut rejected = GameSession::restore(&invalid).expect("ID顺序与时间序不同仍是合法源存档");
    let revision = rejected.external_revision();
    let epoch = rejected.runtime_authority_epoch;
    let install = rejected.runtime_mirror_install_sequence;
    let error = rejected
        .construct_runtime_driver_lazy_for_test()
        .unwrap_err();
    assert!(error.to_string().contains("同价 FIFO"));
    assert!(rejected.require_healthy().is_ok());
    assert!(rejected.runtime_driver.is_none());
    assert_eq!(rejected.external_revision(), revision);
    assert_eq!(rejected.runtime_authority_epoch, epoch);
    assert_eq!(rejected.runtime_mirror_install_sequence, install);
    assert_eq!(
        serde_json::to_value(rejected.save().unwrap()).unwrap(),
        serde_json::to_value(invalid).unwrap()
    );

    source.state.next_order_id = 21;
    let saved = source.save().unwrap();
    let mut first = GameSession::restore(&saved).unwrap();
    first.construct_runtime_driver_lazy_for_test().unwrap();
    let mirrored = first.save().unwrap();
    assert!(mirrored.desk_mirrored_resting);
    assert_eq!(
        mirrored.npc_order_lifecycles.len(),
        2,
        "接管不能靠删除真实 lifecycle 逃过恢复校验"
    );
    for authority in [&saved, &mirrored] {
        let mut missing_all = authority.clone();
        missing_all.resting_orders.clear();
        assert!(
            matches!(GameSession::restore(&missing_all), Err(SessionError::InvalidSave(fault)) if fault.code == 2170),
            "整份在场订单表缺失也必须显式拒绝，不能 panic 或跳过 lifecycle 校验"
        );
        let mut missing = authority.clone();
        missing.npc_order_lifecycles[0].order_id = OrderId(19);
        assert!(
            matches!(GameSession::restore(&missing), Err(SessionError::InvalidSave(fault)) if fault.code == 2170),
            "两种形态的 lifecycle 缺失订单仍必须拒绝"
        );
        let mut wrong_owner = authority.clone();
        wrong_owner.npc_order_lifecycles[0].account = AccountId(3);
        assert!(
            matches!(GameSession::restore(&wrong_owner), Err(SessionError::InvalidSave(fault)) if fault.code == 2170),
            "两种形态的 lifecycle owner 错配仍必须拒绝"
        );
    }
    let mut roundtrip = GameSession::restore(&mirrored).unwrap();
    roundtrip.construct_runtime_driver_lazy_for_test().unwrap();
    for game in [&mut first, &mut roundtrip] {
        {
            let mut driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
            inject(
                &mut driver,
                vec![(AccountId(3), limit(Side::Sell, 990, 100))],
            );
        }
        let events = game.step_via_runtime().unwrap();
        assert!(events.iter().all(|event| !matches!(event, Event::OrderAccepted { id, .. } if [OrderId(7), OrderId(8)].contains(id))),
            "静点接管后的第一拍不能重发已受理旧单事件");
        assert!(game.last_retail_order_events().iter().all(|event| !matches!(event,
            RetailOrderDiagnosticEvent::Submitted { order_id, .. } if [OrderId(7), OrderId(8)].contains(order_id))),
            "静点接管不能把历史旧单伪造为当前 Retail Submitted");
        let buyers = events
            .iter()
            .filter_map(|event| match event {
                Event::Trade {
                    taker,
                    qty: 100,
                    price,
                    ..
                } if price.cents() == 990 => Some(*taker),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            buyers,
            vec![BUYER],
            "真实同价成交必须优先分配给先到 owner2，不能按 ledger owner 键重排"
        );
    }
}
