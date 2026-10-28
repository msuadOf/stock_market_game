//! D1 失败原子性：真实 runtime 事实须等最外成功交付才进入 collector。

use super::*;
use crate::diagnostics::causal::CausalFactKind;

pub(in crate::session) mod retained_tests;
mod rich_drain_tests;

fn game() -> GameSession {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.ticks_per_day = 30;
    GameSession::new(setup, 7).unwrap()
}

fn enqueue(game: &mut GameSession) {
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: StockId(0),
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(995)),
            qty: 100,
        },
    )
    .unwrap();
}

fn driver(game: &mut GameSession) -> crate::runtime::drive::RuntimeDriver {
    let intents = game.take_runtime_player_intents();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(game).unwrap();
    let upper = driver.world_public_views().iter().map(|view| view.daily_upper_limit).collect::<Vec<_>>();
    driver.inject_player_intents(&intents, &upper).unwrap();
    driver
}

fn successful_submission(game: &mut GameSession) {
    game.step().unwrap();
    assert_eq!(
        game.causal_facts()
            .iter()
            .filter(|fact| matches!(fact.kind, CausalFactKind::Submitted(_)))
            .count(),
        1
    );
    assert_eq!(game.causal_facts().len(), 3);
}

#[test]
fn d1_late_callback_failure_preserves_terminal_raw_and_checkpoint_retry_publishes_once() {
    let mut game = game();
    enqueue(&mut game);
    let before = game.business_state_hash().unwrap();
    let checkpoint = game.clone_for_tick_shadow().unwrap();
    let trace_before = serde_json::to_value(&game.state.npc_decision_traces).unwrap();
    let mut called = false;
    let rejected = game.step_frame_and_commit_via_runtime(|_, events| {
        called = true;
        assert!(events
            .iter()
            .any(|fact| matches!(fact.event, Event::OrderAccepted { .. })));
        Err::<(), _>(StepFatal::InvariantViolation {
            location: "d1_callback_failure".to_owned(),
            description: "拒绝候选".to_owned(),
        })
    });
    assert!(called);
    let fatal = rejected.unwrap_err();
    assert_eq!(game.poison_reason(), Some(&fatal));
    let terminal = game.runtime_driver.as_ref().unwrap().lock().unwrap()
        .terminal_causal_snapshot_for_test().expect("必须保留首错 raw 事实");
    assert!(!terminal.1.is_empty());
    assert_eq!(terminal.2, fatal);
    let world_tick = game.runtime_driver.as_ref().unwrap().lock().unwrap().world_committed_tick();
    assert_eq!(game.step_via_runtime().unwrap_err(), fatal);
    assert_eq!(game.runtime_driver.as_ref().unwrap().lock().unwrap().world_committed_tick(), world_tick);
    assert_eq!(game.runtime_driver.as_ref().unwrap().lock().unwrap().terminal_causal_snapshot_for_test(), Some(terminal));
    assert!(game.causal_facts().is_empty());
    assert_eq!(
        trace_before,
        serde_json::to_value(&game.state.npc_decision_traces).unwrap()
    );
    assert_eq!(before, game.business_state_hash().unwrap());
    let mut expected = checkpoint.clone_for_tick_shadow().unwrap();
    expected.poison_runtime_step(fatal);
    assert_eq!(game.session_state_hash().unwrap(), expected.session_state_hash().unwrap());
    let mut game = checkpoint;
    successful_submission(&mut game);
    game.step().unwrap();
    assert_eq!(game.causal_facts().len(), 3);
}

#[test]
fn d1_install_failure_keeps_prior_facts_and_checkpoint_retry_has_no_duplicate() {
    let mut game = game();
    enqueue(&mut game);
    successful_submission(&mut game);
    enqueue(&mut game);
    let saved = game.clone_for_tick_shadow().unwrap();
    let before = serde_json::to_value(game.causal_facts()).unwrap();
    let mut driver = driver(&mut game);
    let before_hash = game.business_state_hash().unwrap();
    game.inject_runtime_install_failure();
    assert!(game.runtime_step_with_install(&mut driver).is_err());
    assert_eq!(serde_json::to_value(game.causal_facts()).unwrap(), before);
    assert_eq!(game.business_state_hash().unwrap(), before_hash);
    game = saved;
    game.step().unwrap();
    assert_eq!(game.causal_facts().len(), 6);
}

#[test]
fn d1_world_validator_abort_keeps_facts_and_successful_retry_publishes_once() {
    let mut game = game();
    enqueue(&mut game);
    let checkpoint = game.clone_for_tick_shadow().unwrap();
    let mut driver = driver(&mut game);
    let before = game.business_state_hash().unwrap();
    driver.set_world_validator_for_test(Box::new(|_| Err("D1 validator拒绝".to_owned())));
    assert!(game.runtime_step_with_install(&mut driver).is_err());
    assert!(game.causal_facts().is_empty());
    assert_eq!(before, game.business_state_hash().unwrap());
    game = checkpoint;
    successful_submission(&mut game);
}

#[test]
fn d1_seal_failure_keeps_facts_and_retry_publishes_once() {
    let mut game = game();
    enqueue(&mut game);
    let checkpoint = game.clone_for_tick_shadow().unwrap();
    let mut driver = driver(&mut game);
    let before = game.business_state_hash().unwrap();
    driver.fail_seal_once_for_test();
    assert!(game.runtime_step_with_install(&mut driver).is_err());
    assert!(game.causal_facts().is_empty());
    assert_eq!(before, game.business_state_hash().unwrap());
    game = checkpoint;
    successful_submission(&mut game);
}

#[test]
fn d1_unresolved_source_before_ordinary_install_preserves_terminal_raw_and_prior_mirror() {
    let mut game = game();
    enqueue(&mut game);
    let checkpoint = game.clone_for_tick_shadow().unwrap();
    let mut driver = driver(&mut game);
    let before_hash = game.business_state_hash().unwrap();
    let mut oracle = game.clone_for_tick_shadow().unwrap();
    driver.unresolved_causal_source_for_test();
    let error = game.runtime_step_with_install(&mut driver).unwrap_err();
    assert!(matches!(error, StepFatal::InvariantViolation { ref location, ref description }
        if location == "runtime::diagnostics::prepare" && description.contains("NonPlayerUnresolved")));
    assert!(game.causal_facts().is_empty());
    assert_eq!(game.business_state_hash().unwrap(), before_hash);
    assert!(!game.runtime_discard_driver_flag, "E2 ordinary terminal 保留 driver 和首错 raw");
    let terminal = driver.terminal_causal_snapshot_for_test().expect("必须保留真实 raw");
    assert!(!terminal.1.is_empty());
    assert_eq!(terminal.2, error);
    let world_tick = driver.world_committed_tick();
    assert_eq!(driver.step_frame_equivalent(&mut game).unwrap_err(), error);
    assert_eq!(driver.world_committed_tick(), world_tick);
    assert_eq!(driver.terminal_causal_snapshot_for_test(), Some(terminal));
    oracle.poison_runtime_step(error);
    assert_eq!(game.session_state_hash().unwrap(), oracle.session_state_hash().unwrap());
    game = checkpoint;
    successful_submission(&mut game);
}

#[test]
fn d1_unresolved_source_at_ordinary_frame_keeps_terminal_driver_before_callback() {
    let mut game = game();
    enqueue(&mut game);
    let before_hash = game.business_state_hash().unwrap();
    let mut oracle = game.clone_for_tick_shadow().unwrap();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut game).unwrap();
    driver.unresolved_causal_source_for_test();
    game.runtime_driver = Some(std::sync::Mutex::new(driver));
    let mut callback = false;
    let result = game.step_frame_and_commit_via_runtime(|_, _| {
        callback = true;
        Ok(())
    });
    let fatal = result.unwrap_err();
    assert!(matches!(&fatal, StepFatal::InvariantViolation { location, description }
        if location == "runtime::diagnostics::prepare" && description.contains("NonPlayerUnresolved")));
    assert!(!callback);
    assert!(game.runtime_driver.is_some(), "保留E2首错 driver");
    let terminal = game.runtime_driver.as_ref().unwrap().lock().unwrap().terminal_causal_snapshot_for_test().unwrap();
    assert!(!terminal.1.is_empty());
    assert_eq!(terminal.2, fatal);
    let world_tick = game.runtime_driver.as_ref().unwrap().lock().unwrap().world_committed_tick();
    assert_eq!(game.step_via_runtime().unwrap_err(), fatal);
    assert_eq!(game.runtime_driver.as_ref().unwrap().lock().unwrap().world_committed_tick(), world_tick);
    assert_eq!(game.runtime_driver.as_ref().unwrap().lock().unwrap().terminal_causal_snapshot_for_test(), Some(terminal));
    oracle.poison_runtime_step(fatal);
    assert_eq!(game.session_state_hash().unwrap(), oracle.session_state_hash().unwrap());
    assert!(game.causal_facts().is_empty());
    assert_eq!(game.business_state_hash().unwrap(), before_hash);
}

#[test]
fn d1_success_consumes_sideband_and_next_tick_cannot_repeat_it() {
    let mut game = game();
    enqueue(&mut game);
    let mut driver = driver(&mut game);
    game.runtime_step_with_install(&mut driver).unwrap();
    assert_eq!(game.causal_facts().len(), 3);
    assert!(driver.world_report_log().iter().all(|record| record.desk_causal.is_empty()));
    game.runtime_step_with_install(&mut driver).unwrap();
    assert_eq!(game.causal_facts().len(), 3);
    assert!(driver.world_report_log().iter().all(|record| record.desk_causal.is_empty()));
}

#[test]
fn d2_real_same_tick_fill_publishes_exact_seller_buyer_and_execution() {
    let mut game = game();
    let seller = AccountId(99);
    let mut account = crate::Account::new(seller, crate::account::AccountKind::Player, Money::from_cents(1_000_000));
    account.fixture_set_positions(std::collections::BTreeMap::from([(
        StockId(0), crate::account::Position::from_restored_parts(100, 0, 100_000, 0),
    )]));
    game.state.accounts.insert(seller, account);
    game.enqueue_player_intent(seller, Intent::PlaceLimit {
        code: StockId(0), side: crate::Side::Sell,
        price: crate::LimitPrice::Fixed(Money::from_cents(995)), qty: 100,
    }).unwrap();
    enqueue(&mut game);
    let events = game.step().expect("真实连续成交必须成功并发布双腿事实");
    assert!(events.iter().any(|event| matches!(event,
        Event::Trade { maker, taker, price, qty: 100, .. }
        if *maker == seller && *taker == AccountId(0) && price.cents() == 995)));
    let order_for = |account| game.causal_facts().iter().find_map(|fact| match &fact.kind {
        CausalFactKind::Submitted(origin) if origin.account == account => Some(origin.order),
        _ => None,
    }).unwrap();
    let (sell, buy) = (order_for(seller), order_for(AccountId(0)));
    let filled = game.causal_facts().iter().filter_map(|fact| match fact.kind {
        CausalFactKind::Filled { order, account, code, qty, value_before, gross } =>
            Some((order, account, code, qty, value_before, gross)),
        _ => None,
    }).collect::<Vec<_>>();
    assert_eq!(filled, vec![(sell, seller, StockId(0), 100, 0, 99_500), (buy, AccountId(0), StockId(0), 100, 0, 99_500)]);
    assert!(game.causal_facts().iter().any(|fact| matches!(fact.kind,
        CausalFactKind::Execution { maker, taker, side: Some(_), qty: 100, price_cents: 995, .. }
        if maker == sell && taker == buy)));
    assert_eq!(game.causal_diagnostics().unwrap().filled_qty, 200);

}

#[test]
fn d3_each_manual_drain_entry_validator_failure_preserves_existing_resting_and_rich_prior() {
    for entry in 0..3 {
        let mut game = game();
        enqueue(&mut game);
        let mut driver = driver(&mut game);
        game.runtime_step_with_install(&mut driver).unwrap();
        let before = game.business_state_hash().unwrap();
        let before_facts = serde_json::to_value(game.causal_facts()).unwrap();
        let hit = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = std::sync::Arc::clone(&hit);
        driver.set_day_end_validator_for_test(Box::new(move |_| {
            observed.store(true, std::sync::atomic::Ordering::SeqCst);
            Err("D3 真实日结 validator 拒绝".to_owned())
        }));
        let error = match entry {
            0 => driver.day_end(&mut game).unwrap_err(),
            1 => driver.day_end_with_runtime_market_day(&mut game).unwrap_err(),
            _ => driver.world_day_end_for_step(&mut game).unwrap_err(),
        };
        assert!(hit.load(std::sync::atomic::Ordering::SeqCst), "必须真实越过 Drain 到达 validator：{error}");
        assert!(error.contains("D3 真实日结 validator 拒绝"), "{error}");
        assert_eq!(game.business_state_hash().unwrap(), before);
        assert_eq!(serde_json::to_value(game.causal_facts()).unwrap(), before_facts);
        assert_eq!(driver.collect_resting_facts().unwrap()[0].len(), 1);
    }
}

#[test]
fn d3_manual_drain_after_checkpoint_rebuild_preserves_original_dayarc_and_real_seeded_orders() {
    let mut game = game();
    enqueue(&mut game);
    successful_submission(&mut game);
    let mut copied = game.clone_for_tick_shadow().unwrap();
    let mut rebuilt = crate::runtime::drive::RuntimeDriver::from_session(&mut copied).unwrap();
    assert!(rebuilt.world_report_log().is_empty(), "重建的发布v0没有旧逐拍记录");
    assert!(rebuilt.committed_calendar_tick().unwrap().is_none());
    let before_facts = serde_json::to_value(copied.causal_facts()).unwrap();
    rebuilt.world_day_end_for_step(&mut copied).expect("原 DayArc 与 World tick 允许新 authority 清理真实旧单");
    assert!(rebuilt.collect_resting_facts_live().unwrap()[0].is_empty());
    assert_eq!(serde_json::to_value(copied.causal_facts()).unwrap(), before_facts, "低层入口不擅自发布 rich collector");
    let (raw, time) = rebuilt.take_drain_causal_facts();
    assert_eq!(raw.len(), 1);
    assert!(matches!(raw[0].kind, crate::runtime::diagnostics::DeskCausalKind::DayEnded { qty: 100, .. }));
    assert_eq!(time.unwrap().civil.date(), copied.civil_date());
    assert!(rebuilt.committed_calendar_tick().unwrap().is_none(), "手动 DayArc 不能补造 TickArc");
}

pub(super) fn d2_game() -> GameSession {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.ticks_per_day = 30;
    let mut second = setup.stocks[0].clone();
    second.code = StockCode("600102".to_owned());
    setup.stocks.push(second);
    let mut game = GameSession::new(setup, 7).unwrap();
    for id in [1, 2] {
        let member = game.join_market(OpaqueSubjectId::new(format!("d2-player-{id}")).unwrap(), true).unwrap();
        assert_eq!(member.account_id, AccountId(id));
        let account = game.state.accounts.get_mut(&member.account_id).unwrap();
        account.fixture_set_positions(std::collections::BTreeMap::from_iter([0, 1].map(|stock| (
            StockId(stock), crate::account::Position::from_restored_parts(1000, 0, 1_000_000, 0),
        ))));
    }
    game
}

fn d2_place(game: &mut GameSession, account: u64, stock: u32, side: crate::Side, price: i64, qty: u32) {
    game.enqueue_player_intent(AccountId(account), Intent::PlaceLimit {
        code: StockId(stock), side, price: crate::LimitPrice::Fixed(Money::from_cents(price)), qty,
    }).unwrap();
}

fn d2_executions(game: &GameSession) -> Vec<(StockId, OrderId, OrderId, crate::Side, u32, i64, crate::diagnostics::causal::Quote)> {
    game.causal_facts().iter().filter_map(|fact| match &fact.kind {
        CausalFactKind::Execution { code, maker, taker, side: Some(side), qty, price_cents, before } =>
            Some((*code, *maker, *taker, *side, *qty, *price_cents, before.clone())),
        _ => None,
    }).collect()
}

#[test]
fn d2_both_active_directions_stock_one_and_same_local_serial_keep_exact_legs() {
    for side in [crate::Side::Buy, crate::Side::Sell] {
        let mut game = d2_game();
        let initial = game.save().unwrap().snapshot.accounts;
        let initial_cash: i64 = initial.values().map(|account| account.cash.cents()).sum();
        let initial_shares: u64 = initial.values().flat_map(|account| account.positions.values()).map(|position| u64::from(position.qty)).sum();
        for stock in [0, 1] {
            if side == crate::Side::Buy { d2_place(&mut game, 2, stock, crate::Side::Sell, 995, 100); }
            else { d2_place(&mut game, 0, stock, crate::Side::Buy, 995, 100); }
        }
        game.step().unwrap();
        for stock in [0, 1] {
            if side == crate::Side::Buy { d2_place(&mut game, 0, stock, side, 995, 100); }
            else { d2_place(&mut game, 2, stock, side, 995, 100); }
        }
        let events = game.step().unwrap();
        let executions = d2_executions(&game);
        assert_eq!(executions.len(), 2);
        for stock in [0, 1] {
            let x = executions.iter().find(|x| x.0 == StockId(stock)).unwrap();
            assert_eq!((x.3, x.4, x.5), (side, 100, 995));
            assert_eq!(x.1.0 >> 32, u64::from(stock));
            assert_eq!(x.2.0 >> 32, u64::from(stock));
            let filled = game.causal_facts().iter().filter_map(|f| match f.kind {
                CausalFactKind::Filled { order, account, code, qty, value_before, gross } if code == StockId(stock) => Some((order, account, qty, value_before, gross)), _ => None,
            }).collect::<Vec<_>>();
            assert_eq!(filled, vec![(x.1, AccountId(2), 100, 0, 99_500), (x.2, AccountId(0), 100, 0, 99_500)]);
            assert!(events.iter().any(|event| matches!(event, Event::Trade { code, maker, taker, qty:100, .. } if *code == StockId(stock) && *maker == AccountId(2) && *taker == AccountId(0))));
        }
        assert_eq!(executions[0].1.0 & u64::from(u32::MAX), executions[1].1.0 & u64::from(u32::MAX));
        assert_eq!(game.causal_diagnostics().unwrap().filled_qty, 400);
        let final_accounts = game.save().unwrap().snapshot.accounts;
        let final_cash: i64 = final_accounts.values().map(|account| account.cash.cents()).sum();
        let fees: i64 = final_accounts.keys().flat_map(|account| game.personal_trade_confirmations_page(*account, None))
            .map(|confirmation| confirmation.actual_fees.total().unwrap().cents()).sum();
        assert!(fees > 0);
        assert_eq!(final_cash + fees, initial_cash, "现金与真实双腿实收费用守恒");
        assert_eq!(final_accounts.values().flat_map(|account| account.positions.values()).map(|position| u64::from(position.qty)).sum::<u64>(), initial_shares);
        assert_eq!(game.account(AccountId(0)).unwrap().sellable_qty(&StockId(1)), 0, "新买股份仍受 T+1 约束");
    }
}

#[test]
fn d2_two_makers_share_incoming_quote_and_next_tick_retains_nonzero_before() {
    let mut game = d2_game();
    d2_place(&mut game, 1, 1, crate::Side::Sell, 990, 100);
    d2_place(&mut game, 2, 1, crate::Side::Sell, 995, 100);
    game.step().unwrap();
    d2_place(&mut game, 0, 1, crate::Side::Buy, 1000, 300);
    game.step().unwrap();
    let first = d2_executions(&game);
    assert_eq!(first.len(), 2);
    assert_eq!((first[0].5, first[1].5), (990, 995));
    assert_eq!(serde_json::to_value(&first[0].6).unwrap(), serde_json::to_value(&first[1].6).unwrap(), "同 incoming 全部 fills 使用同一次操作起点 Quote");
    assert_eq!((first[0].6.ask_depth, first[0].6.ask_cents), (200, Some(990)));
    let buyer = first[0].2;
    assert_eq!(buyer, first[1].2);
    let values = |game: &GameSession| game.causal_facts().iter().filter_map(|f| match f.kind {
        CausalFactKind::Filled { order, value_before, gross, .. } if order == buyer => Some((value_before, gross)), _ => None,
    }).collect::<Vec<_>>();
    assert_eq!(values(&game), vec![(0, 99_000), (99_000, 99_500)]);
    let legacy_before = serde_json::to_value(&game.state.causal).unwrap();
    d2_place(&mut game, 2, 1, crate::Side::Sell, 1000, 100);
    game.step().unwrap();
    assert_eq!(values(&game), vec![(0, 99_000), (99_000, 99_500), (198_500, 100_000)]);
    let legacy_after = serde_json::to_value(&game.state.causal).unwrap();
    assert_eq!(legacy_before["filled_values"], serde_json::json!({}));
    assert_eq!(legacy_before["filled_values"], legacy_after["filled_values"], "native 不增长 legacy tracker");
    assert_eq!(game.causal_diagnostics().unwrap().filled_qty, 600);
}

#[test]
fn d2_driver_adopt_preserves_partial_order_real_cumulative_value() {
    let mut game = d2_game();
    d2_place(&mut game, 2, 1, crate::Side::Sell, 995, 100);
    game.step().unwrap();
    d2_place(&mut game, 0, 1, crate::Side::Buy, 1000, 200);
    game.step().unwrap();
    let before_submitted = game.causal_facts().iter().filter(|f| matches!(f.kind, CausalFactKind::Submitted(_))).count();
    let mut adopted = game.clone_for_tick_shadow().unwrap();
    d2_place(&mut adopted, 2, 1, crate::Side::Sell, 1000, 100);
    let mut rebuilt = driver(&mut adopted);
    adopted.runtime_step_with_install(&mut rebuilt).unwrap();
    assert!(adopted.causal_facts().iter().any(|f| matches!(f.kind, CausalFactKind::Filled { account: AccountId(0), value_before: 99_500, gross:100_000, .. })));
    assert_eq!(adopted.causal_facts().iter().filter(|f| matches!(f.kind, CausalFactKind::Submitted(_))).count(), before_submitted + 1);
}

#[test]
fn d2_nonempty_fill_rollback_plain_install_seal_validator_and_frame_callback() {
    for failure in 0..4 {
        let mut game = d2_game();
        d2_place(&mut game, 2, 1, crate::Side::Sell, 995, 200);
        game.step().unwrap();
        assert!(!game.causal_facts().is_empty());
        d2_place(&mut game, 0, 1, crate::Side::Buy, 995, 100);
        let checkpoint = game.clone_for_tick_shadow().unwrap();
        let facts = serde_json::to_value(game.causal_facts()).unwrap();
        let business;
        let mut candidate_called = false;
        if failure == 3 {
            business = game.business_state_hash().unwrap();
            let error = game.step_frame_and_commit_via_runtime(|_, events| {
                candidate_called = true;
                assert!(events.iter().any(|f| matches!(f.event, Event::Trade { qty:100, .. })));
                Err::<(),_>(StepFatal::InvariantViolation { location:"d2 callback".to_owned(), description:"拒绝真实成交候选".to_owned() })
            });
            assert!(error.is_err());
            assert!(candidate_called);
        } else {
            let mut runtime = driver(&mut game);
            // 手动入口在意图已冻结取出后开始；完整 hash 须取相同权威边界。
            business = game.business_state_hash().unwrap();
            match failure {
                0 => game.inject_runtime_install_failure(),
                1 => runtime.fail_seal_once_for_test(),
                _ => runtime.set_world_validator_for_test(Box::new(|evidence| {
                    assert_eq!(evidence.desk_trades.len(), 1);
                    Err("拒绝真实成交".to_owned())
                })),
            }
            assert!(game.runtime_step_with_install(&mut runtime).is_err());
        }
        assert_eq!(serde_json::to_value(game.causal_facts()).unwrap(), facts);
        assert_eq!(game.business_state_hash().unwrap(), business, "poison metadata 不属于业务 hash");
        game = checkpoint;
        game.step().unwrap();
        assert_eq!(d2_executions(&game).len(), 1, "健康 checkpoint retry 仅发布一次");
        assert_eq!(game.causal_diagnostics().unwrap().filled_qty, 200);
        let query_hash = game.business_state_hash().unwrap();
        let saved = serde_json::to_value(game.save().unwrap()).unwrap();
        game.causal_diagnostics().unwrap();
        assert_eq!(query_hash, game.business_state_hash().unwrap());
        assert_eq!(saved, serde_json::to_value(game.save().unwrap()).unwrap());
    }
}

#[test]
fn d2_restore_partial_order_keeps_nonzero_before_without_fabricated_origin() {
    let mut game = d2_game();
    d2_place(&mut game, 2, 1, crate::Side::Sell, 995, 100);
    game.step().unwrap();
    d2_place(&mut game, 0, 1, crate::Side::Buy, 1000, 200);
    game.step().unwrap();
    let slot = game.save().unwrap();
    let partial = slot.runtime_state.live_envelopes.iter().find(|live| live.key.account == AccountId(0)).unwrap();
    assert_eq!((partial.remaining_qty, partial.filled_qty, partial.filled_value.cents()), (100, 100, 99_500));
    let mut restored = GameSession::restore(&slot).unwrap();
    assert!(matches!(restored.causal_diagnostics(), Err(crate::diagnostics::causal::CausalError::RestoredObservation)));
    d2_place(&mut restored, 2, 1, crate::Side::Sell, 1000, 100);
    restored.step().unwrap();
    d2_place(&mut game, 2, 1, crate::Side::Sell, 1000, 100);
    game.step().unwrap();
    assert_eq!(serde_json::to_value(game.save().unwrap().snapshot.accounts).unwrap(),
        serde_json::to_value(restored.save().unwrap().snapshot.accounts).unwrap(), "恢复续成交保持真实费用、现金、持仓与 T+1");
    for account in [AccountId(0), AccountId(2)] {
        let continuous = game.personal_trade_confirmations_page(account, None);
        let resumed = restored.personal_trade_confirmations_page(account, None);
        assert_eq!(continuous[0].actual_fees, resumed[0].actual_fees, "双腿累计费用续链不归零");
    }
    assert!(restored.causal_facts().iter().any(|f| matches!(f.kind, CausalFactKind::Filled { account: AccountId(0), value_before:99_500,gross:100_000,.. })));
    assert_eq!(restored.causal_facts().iter().filter(|f| matches!(f.kind,CausalFactKind::Submitted(_))).count(),1);
    assert!(matches!(restored.causal_diagnostics(), Err(crate::diagnostics::causal::CausalError::RestoredObservation)));
}

#[cfg(feature = "verification-harness")]
#[test]
fn d2_protocol_checkpoint_after_real_fill_restores_exact_facts_sequence_and_queries() {
    let mut game=d2_game();
    d2_place(&mut game,2,1,crate::Side::Sell,995,300);
    game.step().unwrap();
    let mut protocol=crate::session::protocol::ProtocolSession::from_game_for_verification(game,None);
    let buy=|| Intent::PlaceLimit {code:StockId(1),side:crate::Side::Buy,price:crate::LimitPrice::Fixed(Money::from_cents(995)),qty:100};
    protocol.enqueue_player_intent(AccountId(0),buy()).unwrap();protocol.step_frame().unwrap();
    let checkpoint=protocol.checkpoint().unwrap();
    let before=serde_json::to_value(protocol.game().causal_facts()).unwrap();
    let hash=protocol.game().business_state_hash().unwrap();
    protocol.enqueue_player_intent(AccountId(0),buy()).unwrap();
    // 只在独立 reader 观察已受理输入，不推进真实入口的 cutoff。
    let observe_receipts = |protocol: &crate::session::protocol::ProtocolSession| {
        let mut observed = protocol.game().clone_for_tick_shadow().unwrap();
        observed.freeze_shared_ingress().unwrap();
        observed.state.pending_player.clone()
    };
    let received = observe_receipts(&protocol);
    assert_eq!(received.len(), 1);
    assert_eq!((received[0].owner, received[0].account_ordinal, received[0].stock_ordinal), (AccountId(0), 1, 2));
    let second_frame = serde_json::to_value(protocol.step_frame().unwrap()).unwrap();
    let second_facts = serde_json::to_value(protocol.game().causal_facts()).unwrap();
    let second_hash = protocol.game().business_state_hash().unwrap();
    protocol.step_frame().unwrap();
    assert_eq!(d2_executions(protocol.game()).len(),2);
    protocol.rollback(checkpoint);
    assert_eq!(serde_json::to_value(protocol.game().causal_facts()).unwrap(),before);
    assert_eq!(protocol.game().business_state_hash().unwrap(),hash);
    assert_eq!(protocol.game().causal_diagnostics().unwrap().filled_qty,200);
    // checkpoint 不撤销外部受理事实；重放原 receipt，不能再次入队同一买单。
    assert_eq!(serde_json::to_value(observe_receipts(&protocol)).unwrap(), serde_json::to_value(received).unwrap());
    assert_eq!(serde_json::to_value(protocol.step_frame().unwrap()).unwrap(), second_frame);
    assert_eq!(serde_json::to_value(protocol.game().causal_facts()).unwrap(), second_facts);
    assert_eq!(protocol.game().business_state_hash().unwrap(), second_hash);
    assert_eq!(d2_executions(protocol.game()).len(),2);
    assert!(protocol.game().causal_facts().iter().enumerate().all(|(index,f)|f.sequence==index as u64));
}

#[test]
fn d2_prepare_market_after_real_fill_restores_both_install_capture_modes() {
    for capture in [false, true] {
        let mut game = d2_game();
        d2_place(&mut game, 2, 1, crate::Side::Sell, 995, 200);
        game.step().unwrap();
        let checkpoint = game.clone_for_tick_shadow().unwrap();
        let prior_facts = serde_json::to_value(game.causal_facts()).unwrap();
        game.enqueue_player_intent(AccountId(0), Intent::PlaceMarket { code: StockId(1), side: crate::Side::Buy, qty: 100 }).unwrap();
        let mut runtime = driver(&mut game);
        let before = game.business_state_hash().unwrap();
        let mut poison_oracle = game.clone_for_tick_shadow().unwrap();
        let error = if capture {
            game.runtime_step_with_prepared_mirror(&mut runtime, |_, facts| {
                Ok(facts.into_iter().map(|fact| fact.event).collect::<Vec<_>>())
            }).err().unwrap()
        } else {
            game.runtime_step_with_install(&mut runtime).err().unwrap()
        };
        assert!(matches!(error, StepFatal::InvariantViolation { ref location, ref description }
            if location == "runtime::diagnostics::prepare" && description.contains("Market")));
        assert_eq!(runtime.world_report_log().last().unwrap().desk_trades.len(), 1, "prepare 拒绝前真实 World 已产生一笔双腿成交");
        assert_eq!(serde_json::to_value(game.causal_facts()).unwrap(), prior_facts);
        assert_eq!(game.business_state_hash().unwrap(), before);
        // E2 已退役 returning-baseline；两个聚合入口均保留 terminal driver 与首错 raw。
        assert!(!game.runtime_discard_driver_flag, "E2 两入口均须保留 terminal raw");
        let terminal = runtime.terminal_causal_snapshot_for_test().unwrap();
        assert!(!terminal.1.is_empty());
        assert_eq!(terminal.2, error);
        let world_tick = runtime.world_committed_tick();
        assert_eq!(runtime.step_frame_equivalent(&mut game).unwrap_err(), error);
        assert_eq!(runtime.world_committed_tick(), world_tick);
        assert_eq!(runtime.terminal_causal_snapshot_for_test(), Some(terminal));
        poison_oracle.poison_runtime_step(error);
        assert_eq!(game.session_state_hash().unwrap(), poison_oracle.session_state_hash().unwrap());
        game = checkpoint;
        d2_place(&mut game, 0, 1, crate::Side::Buy, 995, 100);
        game.step().unwrap();
        assert_eq!(d2_executions(&game).len(), 1);
        assert_eq!(game.causal_diagnostics().unwrap().filled_qty, 200);
    }
}

mod coordinator_source_tests;

#[test]
fn d2b_rich_desk_reject_is_explicit_unsupported_and_keeps_prior_candidate_unpublished() {
    let mut game = d2_game();
    d2_place(&mut game, 0, 1, crate::Side::Buy, 990, 100);
    game.step_via_runtime().unwrap();
    d2_place(&mut game, 0, 1, crate::Side::Buy, 1050, 100);
    let mut checkpoint = game.clone_for_tick_shadow().unwrap();
    let prior_facts = serde_json::to_value(game.causal_facts()).unwrap();
    let prior_events = game.last_retail_order_events().to_vec();
    let prior_decisions = game.last_retail_decisions().to_vec();
    let error = game.step_via_runtime().unwrap_err();
    assert!(matches!(&error, StepFatal::InvariantViolation { location, description }
        if location == "runtime::diagnostics::prepare" && description.contains("真实 desk reject 的 rich 来源尚未接通")));
    assert_eq!(serde_json::to_value(game.causal_facts()).unwrap(), prior_facts);
    assert_eq!(game.last_retail_order_events(), prior_events);
    assert_eq!(game.last_retail_decisions(), prior_decisions);
    assert_eq!(game.business_state_hash().unwrap(), checkpoint.business_state_hash().unwrap());
    checkpoint.poison_runtime_step(error);
    assert_eq!(game.session_state_hash().unwrap(), checkpoint.session_state_hash().unwrap());
}

#[test]
fn d2b_feature_cost_300_100_200_uses_same_minimal_report_and_releases_record_copy() {
    let mut game = d2_game();
    let cost = crate::runtime::retail_lifecycle::cost_probe::Arm::new();
    d2_place(&mut game, 2, 1, crate::Side::Sell, 995, 100);
    d2_place(&mut game, 0, 1, crate::Side::Buy, 995, 300);
    game.enqueue_player_intent(AccountId(0), Intent::Cancel { code:StockId(1), id:OrderId((1_u64<<32)|2) }).unwrap();
    let events = game.step_via_runtime().unwrap();
    assert_eq!(events.iter().filter(|event| matches!(event, Event::Trade { qty:100, .. })).count(), 1);
    assert_eq!(game.causal_diagnostics().unwrap().filled_qty, 200);
    let reports = cost.take();
    assert_eq!(reports.len(), 1);
    let report = &reports[0];
    assert_eq!((report.raw_len, report.resolved_len), (4, 5));
    assert!(report.raw_capacity <= 8 && report.resolved_capacity <= 8);
    assert_eq!((report.raw_element_bytes, report.resolved_element_bytes), (32, 56));
    let driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
    assert!(driver.world_report_log().iter().all(|record| record.retail.is_empty()));
    println!("D2b feature 300/100/200 report cost: {reports:?}");
}
