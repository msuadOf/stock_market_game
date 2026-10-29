//! 剩余 rich 生命周期的 tests-first 输入；只使用真实 Player、原委托与原 World 报告。

use super::*;
use crate::diagnostics::causal::Termination;
use crate::runtime::LifecycleKind;

const BUYER: AccountId = AccountId(0);
const SELLER: AccountId = AccountId(2);
const CODE: StockId = StockId(1);

fn market(game: &mut GameSession, qty: u32) {
    game.enqueue_player_intent(
        BUYER,
        Intent::PlaceMarket {
            code: CODE,
            side: Side::Buy,
            qty,
        },
    )
    .expect("真实 Player 有足够现金，市价意图合法受理");
}

fn assert_target_market_failure(error: &StepFatal) {
    assert!(
        matches!(error, StepFatal::InvariantViolation { location, description }
        if location == "runtime::diagnostics::prepare"
            && description.contains("Market或PreOpen申报生命周期尚未接通")),
        "非目标失败不能认证为 Market 业务首红：{error:?}"
    );
}

#[test]
fn d3_rich_two_empty_market_orders_keep_real_ids_and_end_only_their_remainders() {
    let mut game = d2_game();
    let initial_cash = game.account(BUYER).unwrap().cash();
    market(&mut game, 100);
    market(&mut game, 100);
    let mut runtime = driver(&mut game);
    let outcome = game.runtime_step_with_install(&mut runtime);
    // 即使 prepare 尚未接线而拒绝，首红也必须先证明 World 真处理了两笔即时单。
    let record = runtime
        .world_report_log()
        .last()
        .expect("原 World 已成功收束");
    assert!(record.desk_trades.is_empty());
    assert!(record.desk_receipts.is_empty());
    assert!(record.group_rejections.is_empty());
    assert_eq!(record.desk_lifecycle.len(), 2);
    for fact in &record.desk_lifecycle {
        assert_eq!(
            (u32::from(fact.stock), fact.remaining, fact.side, fact.kind),
            (CODE.0, 100, Side::Buy, LifecycleKind::Cancelled)
        );
        assert_eq!(fact.owner, record.desk_lifecycle[0].owner);
        assert_eq!(runtime.account_by_owner(fact.owner), Some(BUYER));
    }
    let ids = record
        .desk_lifecycle
        .iter()
        .map(|fact| OrderId(fact.order_serial))
        .collect::<Vec<_>>();
    assert_eq!(ids[0].0 >> 32, u64::from(CODE.0));
    assert_eq!(
        ids[1].0,
        ids[0].0 + 1,
        "每笔即时单真实消费序号，不复用 OrderId"
    );
    if let Err(error) = &outcome {
        assert_target_market_failure(error);
    }
    let events =
        outcome.expect("真实空簿市价应发布 Submitted 与 MarketRemainder，不能停在 Unsupported");
    assert!(!events
        .iter()
        .any(|event| matches!(event, Event::Trade { .. } | Event::OrderAccepted { .. })));
    for id in &ids {
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::OrderCanceled {
            account: BUYER, code: CODE, id: actual, remaining_qty: 100, ..
        } if actual == id))
                .count(),
            1
        );
    }
    let report = game.causal_diagnostics().unwrap();
    assert_eq!(report.orders.len(), 2);
    assert_eq!(
        (
            report.submitted_qty,
            report.filled_qty,
            report.canceled_qty,
            report.open_qty
        ),
        (200, 0, 200, 0)
    );
    for row in &report.orders {
        assert!(ids.contains(&row.origin.order));
        assert_eq!(
            (row.origin.account, row.origin.code, row.origin.qty),
            (BUYER, CODE, 100)
        );
        assert_eq!(
            (row.origin.company, row.origin.plan, row.origin.decision),
            (game.state.issuer_of_stock(CODE), None, None)
        );
        assert_eq!(
            (
                row.filled_qty,
                row.canceled_qty,
                row.aborted_qty,
                row.open_qty
            ),
            (0, 100, 0, 0)
        );
        assert_eq!(row.terminal_reason, Some(Termination::MarketRemainder));
    }
    assert!(!game.causal_facts().iter().any(|fact| matches!(
        fact.kind,
        CausalFactKind::Filled { .. } | CausalFactKind::Execution { .. }
    )));
    let committed = runtime.committed_calendar_tick().unwrap().unwrap();
    let before = committed.calendar.before_projection();
    for fact in game.causal_facts() {
        assert_eq!(
            fact.time.market_minute,
            before.completed_shared_minute_count()
        );
        let clock = fact
            .time
            .stock_clock
            .expect("真实空簿市价事实也须携本股 clock");
        assert_eq!(clock.code, CODE);
        assert_eq!(
            clock.completed_minutes,
            before.stock(CODE).unwrap().completed_minute_count()
        );
        assert_eq!(fact.time.phase, committed.calendar.phase_before());
        assert_eq!(
            fact.time.civil,
            committed.calendar.observation_instant_after()
        );
    }
    assert_eq!(
        game.account(BUYER).unwrap().cash(),
        initial_cash,
        "无成交必须完整退回原冻结现金"
    );
    assert!(runtime
        .collect_resting_facts_live()
        .unwrap()
        .iter()
        .all(Vec::is_empty));
}

#[test]
fn d3_rich_market_partial_fill_preserves_real_before_and_full_fill_has_no_remainder() {
    for submitted in [300, 100] {
        let mut game = d2_game();
        let initial = game.save().unwrap().snapshot.accounts;
        d2_place(&mut game, SELLER.0, CODE.0, Side::Sell, 995, 100);
        let mut runtime = driver(&mut game);
        let first = game
            .runtime_step_with_install(&mut runtime)
            .expect("真实旧仓先挂卖单 100 股");
        let sell = first
            .iter()
            .find_map(|event| match event {
                Event::OrderAccepted {
                    account: SELLER,
                    code: CODE,
                    id,
                    remaining_qty: 100,
                    ..
                } => Some(*id),
                _ => None,
            })
            .expect("必须有原卖单 OrderAccepted，不能合成对手单");
        let prefix = serde_json::to_value(game.causal_facts()).unwrap();
        let previous = game.causal_facts().len();
        market(&mut game, submitted);
        let intents = game.take_runtime_player_intents();
        let upper = runtime
            .world_public_views()
            .iter()
            .map(|view| view.daily_upper_limit)
            .collect::<Vec<_>>();
        runtime
            .inject_player_intents(&intents, &upper)
            .expect("同一外持 runtime 接收本次唯一真实 Market receipt，不重新接管 Session");
        let outcome = game.runtime_step_with_install(&mut runtime);
        let record = runtime
            .world_report_log()
            .last()
            .expect("原 World 必须已发生真实成交");
        assert!(record.desk_receipts.is_empty());
        assert!(record.group_rejections.is_empty());
        assert_eq!(record.desk_trades.len(), 1);
        let trade = record.desk_trades[0];
        assert_eq!(runtime.account_by_owner(trade.maker), Some(SELLER));
        assert_eq!(runtime.account_by_owner(trade.taker), Some(BUYER));
        assert_eq!(
            (
                u32::from(trade.stock),
                trade.qty,
                trade.price,
                trade.taker_side
            ),
            (CODE.0, 100, 995, Side::Buy)
        );
        assert_eq!(trade.maker_order_serial, sell.0);
        let buy = OrderId(trade.taker_order_serial);
        assert_ne!(sell, buy);
        assert_eq!(record.desk_lifecycle.len(), usize::from(submitted > 100));
        if submitted > 100 {
            let remainder = record.desk_lifecycle[0];
            assert_eq!((u32::from(remainder.stock), remainder.owner, remainder.side),
                (CODE.0, trade.taker, Side::Buy));
            assert_eq!(
                (remainder.order_serial, remainder.remaining, remainder.kind),
                (buy.0, 200, LifecycleKind::Cancelled)
            );
        }
        if let Err(error) = &outcome {
            assert_target_market_failure(error);
        }
        let events = outcome.expect("真实市价双腿和余量应成功发布，不能停在 Market Unsupported");
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::Trade {
            code: CODE, maker: SELLER, taker: BUYER, qty: 100, price, ..
        } if price.cents() == 995))
                .count(),
            1
        );
        assert_eq!(
            serde_json::to_value(&game.causal_facts()[..previous]).unwrap(),
            prefix
        );
        let facts = &game.causal_facts()[previous..];
        let executions = d2_executions(&game);
        assert_eq!(executions.len(), 1);
        let execution = &executions[0];
        assert_eq!(
            (
                execution.0,
                execution.1,
                execution.2,
                execution.3,
                execution.4,
                execution.5
            ),
            (CODE, sell, buy, Side::Buy, 100, 995)
        );
        assert_eq!(
            (
                execution.6.bid_cents,
                execution.6.ask_cents,
                execution.6.bid_depth,
                execution.6.ask_depth
            ),
            (None, Some(995), 0, 100),
            "before 必须是成交前的真实内部盘口"
        );
        let filled = facts
            .iter()
            .filter_map(|fact| match fact.kind {
                CausalFactKind::Filled {
                    order,
                    account,
                    code: CODE,
                    qty,
                    value_before,
                    gross,
                } => Some((order, account, qty, value_before, gross)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            filled,
            vec![(sell, SELLER, 100, 0, 99_500), (buy, BUYER, 100, 0, 99_500)]
        );
        let report = game.causal_diagnostics().unwrap();
        let row = report
            .orders
            .iter()
            .find(|row| row.origin.order == buy)
            .unwrap();
        assert_eq!(
            (
                row.origin.qty,
                row.filled_qty,
                row.canceled_qty,
                row.aborted_qty,
                row.open_qty
            ),
            (submitted, 100, u64::from(submitted - 100), 0, 0)
        );
        assert_eq!(
            row.terminal_reason,
            (submitted > 100).then_some(Termination::MarketRemainder)
        );
        assert_eq!(facts.iter().filter(|fact| matches!(fact.kind, CausalFactKind::Terminated { order, .. } if order == buy)).count(),
            usize::from(submitted > 100), "全成不得再发布余量终止");
        let committed = runtime.committed_calendar_tick().unwrap().unwrap();
        let before = committed.calendar.before_projection();
        for fact in facts {
            assert_eq!(
                fact.time.market_minute,
                before.completed_shared_minute_count()
            );
            let clock = fact
                .time
                .stock_clock
                .expect("真实市价证券事实须携本股 clock");
            assert_eq!(clock.code, CODE);
            assert_eq!(
                clock.completed_minutes,
                before.stock(CODE).unwrap().completed_minute_count()
            );
            assert_eq!(
                fact.time.civil,
                committed.calendar.observation_instant_after()
            );
            assert_eq!(fact.time.phase, committed.calendar.phase_before());
        }
        assert!(runtime
            .collect_resting_facts_live()
            .unwrap()
            .iter()
            .all(Vec::is_empty));
        let final_accounts = game.save().unwrap().snapshot.accounts;
        assert_eq!(
            final_accounts[&BUYER].cash.cents(),
            initial[&BUYER].cash.cents() - 99_500 - trade.taker_fees_cents,
            "市价保护价预留及余量必须退回原买方，只扣真实成交与买腿实收费"
        );
        assert_eq!(
            final_accounts[&SELLER].cash.cents(),
            initial[&SELLER].cash.cents() + 99_500 - trade.maker_fees_cents,
            "原卖方只收真实成交减卖腿实收费，不能误收买方余量退款"
        );
        assert!(
            initial[&BUYER].positions.get(&CODE).is_none(),
            "原买方无该股持仓"
        );
        assert_eq!(
            final_accounts[&BUYER].positions[&CODE].qty, 100,
            "实际股份必须进入原买方账户"
        );
        assert_eq!(
            final_accounts[&BUYER].positions[&CODE].t1_locked, 100,
            "新增买入100股全部受T+1锁定"
        );
        assert_eq!(
            final_accounts[&SELLER].positions[&CODE].qty,
            initial[&SELLER].positions[&CODE].qty - 100,
            "只从真实原卖方扣除成交100股"
        );
        let initial_cash = initial
            .values()
            .map(|account| account.cash.cents())
            .sum::<i64>();
        let final_cash = final_accounts
            .values()
            .map(|account| account.cash.cents())
            .sum::<i64>();
        let fees = final_accounts
            .keys()
            .flat_map(|account| game.personal_trade_confirmations_page(*account, None))
            .map(|confirmation| confirmation.actual_fees.total().unwrap().cents())
            .sum::<i64>();
        assert!(fees > 0);
        assert_eq!(final_cash + fees, initial_cash);
        let initial_shares = initial
            .values()
            .flat_map(|account| account.positions.values())
            .map(|position| u64::from(position.qty))
            .sum::<u64>();
        let final_shares = final_accounts
            .values()
            .flat_map(|account| account.positions.values())
            .map(|position| u64::from(position.qty))
            .sum::<u64>();
        assert_eq!(final_shares, initial_shares);
        assert_eq!(
            game.account(BUYER).unwrap().sellable_qty(&CODE),
            0,
            "真实买入继续受 T+1 约束"
        );
    }
}
