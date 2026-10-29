#![cfg(feature = "simulation-diagnostics")]

#[path = "diagnostic_parity.rs"]
mod fixture;

use engine::diagnostics::causal::{CausalFactKind, Termination};
use engine::session::protocol::ProtocolSession;
use engine::{AccountId, Event, GameSession, Intent, LimitPrice, Money, Side, StockId};

fn setup() -> engine::SessionSetup {
    let mut setup = fixture::setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup
}

fn place(code: StockId) -> Intent {
    Intent::PlaceLimit {
        code,
        side: Side::Buy,
        price: LimitPrice::Fixed(Money::from_cents(995)),
        qty: 100,
    }
}

#[test]
fn d1_real_lifecycle_keeps_original_quantity_order_quotes_and_exactly_once() {
    let mut game = GameSession::new(setup(), 7).unwrap();
    let code = game.stock_id("600101").unwrap();
    game.enqueue_player_intent(AccountId(0), place(code))
        .unwrap();
    let events = game.step().unwrap();
    let order = events
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .unwrap();
    let facts = game.causal_facts();
    assert_eq!(facts.len(), 3, "pre Quote → Submitted → post Quote");
    assert!(
        matches!(&facts[0].kind, CausalFactKind::Quote(q) if q.bid_depth == 0 && q.ask_depth == 0)
    );
    assert!(
        matches!(&facts[1].kind, CausalFactKind::Submitted(o) if o.order == order && o.qty == 100 && o.side == Side::Buy && o.plan.is_none() && o.decision.is_none())
    );
    assert!(
        matches!(&facts[2].kind, CausalFactKind::Quote(q) if q.bid_cents == Some(995) && q.bid_depth == 100 && q.ask_depth == 0)
    );
    game.enqueue_player_intent(AccountId(0), Intent::Cancel { code, id: order })
        .unwrap();
    game.step().unwrap();
    let facts = game.causal_facts();
    assert_eq!(facts.len(), 5);
    assert!(
        matches!(&facts[3].kind, CausalFactKind::Terminated { order: id, qty: 100, reason: Termination::Voluntary, .. } if *id == order)
    );
    assert!(
        matches!(&facts[4].kind, CausalFactKind::Quote(q) if q.bid_cents.is_none() && q.bid_depth == 0)
    );
    assert_eq!(facts[3].time.market_minute - facts[1].time.market_minute, 8);
    let committed = serde_json::to_value(game.causal_facts()).unwrap();
    game.enqueue_player_intent(AccountId(0), Intent::Cancel { code, id: order })
        .unwrap();
    game.step().unwrap();
    game.step().unwrap();
    assert_eq!(
        committed,
        serde_json::to_value(game.causal_facts()).unwrap(),
        "失败撤单和空拍不得重复发布"
    );
    let report = game.causal_diagnostics().unwrap();
    assert_eq!(
        (report.submitted_qty, report.canceled_qty, report.open_qty),
        (100, 100, 0)
    );
    assert_eq!(report.orders[0].lifetime_civil_seconds, Some(480));
}

#[test]
fn d1_owned_protocol_checkpoints_restore_multiple_successes_across_sessions() {
    // 此处验证 serial owned checkpoint，证据入口明确不创建 live runtime authority。
    let mut session = ProtocolSession::new(setup(), 7).unwrap();
    let code = session.game().stock_id("600101").unwrap();
    session
        .enqueue_player_intent(AccountId(0), place(code))
        .unwrap();
    let first = session.step_frame_with_commit_evidence().map(|(frame, _)| frame).unwrap();
    let order = first
        .facts
        .iter()
        .find_map(|fact| match fact.event {
            Event::OrderAccepted { id, .. } => Some(id),
            _ => None,
        })
        .unwrap();
    assert_eq!(session.game().causal_facts().len(), 3);
    let first_checkpoint = session.checkpoint().unwrap();
    let first_facts = serde_json::to_value(session.game().causal_facts()).unwrap();
    let first_hash = session.game().business_state_hash().unwrap();
    session
        .enqueue_player_intent(AccountId(0), Intent::Cancel { code, id: order })
        .unwrap();
    session.step_frame_with_commit_evidence().map(|(frame, _)| frame).unwrap();
    let second_checkpoint = session.checkpoint().unwrap();
    let second_facts = serde_json::to_value(session.game().causal_facts()).unwrap();
    session.step_frame_with_commit_evidence().map(|(frame, _)| frame).unwrap();
    session.step_frame_with_commit_evidence().map(|(frame, _)| frame).unwrap();
    session.rollback(first_checkpoint).unwrap();
    assert_eq!(
        serde_json::to_value(session.game().causal_facts()).unwrap(),
        first_facts
    );
    assert_eq!(session.game().business_state_hash().unwrap(), first_hash);
    let mut other = ProtocolSession::new(setup(), 99).unwrap();
    other.rollback(second_checkpoint).unwrap();
    assert_eq!(
        serde_json::to_value(other.game().causal_facts()).unwrap(),
        second_facts
    );
    let before_hash = other.game().business_state_hash().unwrap();
    let before_save = serde_json::to_value(other.game().save().unwrap()).unwrap();
    other.game().causal_diagnostics().unwrap();
    assert_eq!(other.game().business_state_hash().unwrap(), before_hash);
    assert_eq!(
        serde_json::to_value(other.game().save().unwrap()).unwrap(),
        before_save
    );
    other.step_frame_with_commit_evidence().map(|(frame, _)| frame).unwrap();
    assert_eq!(
        serde_json::to_value(other.game().causal_facts()).unwrap(),
        second_facts
    );
}

#[test]
fn d1_quote_depth_uses_the_complete_single_stock_book() {
    let mut game = GameSession::new(setup(), 7).unwrap();
    let code = game.stock_id("600101").unwrap();
    for price in 990..998 {
        game.enqueue_player_intent(AccountId(0), Intent::PlaceLimit {
            code, side: Side::Buy, price: LimitPrice::Fixed(Money::from_cents(price)), qty: 100,
        }).unwrap();
    }
    let started = std::time::Instant::now();
    game.step().unwrap();
    let elapsed = started.elapsed();
    assert_eq!(game.causal_facts().len(), 24);
    assert!(matches!(&game.causal_facts()[23].kind, CausalFactKind::Quote(quote) if quote.bid_depth == 800 && quote.bid_cents == Some(997)));
    println!("D1 单股8档/8次原始申报及16次精确Quote采样 wall={elapsed:?}；采样成本为O(本股挂单数)");
}

#[test]
fn d1_unsupported_market_operation_is_explicit_and_rolls_back() {
    // 保留历史 exact 名称；Market 已接通，改核原公开入口的真实余量与退款。
    let mut game = GameSession::new(setup(), 7).unwrap();
    let code = game.stock_id("600101").unwrap();
    let initial_cash = game.account(AccountId(0)).unwrap().cash();
    let before_minute = game.current_market_minute();
    let before_phase = game.phase();
    game.enqueue_player_intent(AccountId(0), Intent::PlaceMarket { code, side: Side::Buy, qty: 100 }).unwrap();
    let events = game.step().expect("空簿市价必须发布真实余量撤销");
    let canceled = events.iter().filter_map(|event| match event {
        Event::OrderCanceled { account: AccountId(0), code: actual, id, remaining_qty: 100, .. }
            if *actual == code => Some(*id),
        _ => None,
    }).collect::<Vec<_>>();
    assert_eq!(canceled.len(), 1, "原100股即时单恰好撤销一次");
    let order = canceled[0];
    assert!(!events.iter().any(|event| matches!(event, Event::Trade { .. } | Event::OrderAccepted { .. })));
    let report = game.causal_diagnostics().unwrap();
    assert_eq!((report.submitted_qty, report.filled_qty, report.canceled_qty, report.open_qty), (100, 0, 100, 0));
    assert_eq!(report.orders.len(), 1);
    let row = &report.orders[0];
    assert_eq!((row.origin.order, row.origin.account, row.origin.code, row.origin.qty), (order, AccountId(0), code, 100));
    assert_eq!((row.origin.plan, row.origin.decision), (None, None));
    assert_eq!(row.terminal_reason, Some(Termination::MarketRemainder));
    let facts = game.causal_facts();
    assert_eq!(facts.iter().filter(|fact| matches!(fact.kind, CausalFactKind::Submitted(_))).count(), 1);
    assert_eq!(facts.iter().filter(|fact| matches!(fact.kind, CausalFactKind::Terminated {
        order: actual, account: AccountId(0), code: actual_code, qty: 100, reason: Termination::MarketRemainder,
    } if actual == order && actual_code == code)).count(), 1);
    assert!(!facts.iter().any(|fact| matches!(fact.kind, CausalFactKind::Filled { .. } | CausalFactKind::Execution { .. })));
    for fact in facts {
        assert_eq!(fact.time.market_minute, before_minute);
        let clock = fact.time.stock_clock.expect("真实证券事实必须有本股时钟");
        assert_eq!((clock.code, clock.completed_minutes), (code, before_minute));
        assert_eq!(fact.time.phase, before_phase);
        assert_eq!(fact.time.civil, game.runtime_observation_instant());
    }
    assert_eq!(game.account(AccountId(0)).unwrap().cash(), initial_cash, "无成交原现金须全部退款");
    assert!(game.save().unwrap().resting_orders.values().all(Vec::is_empty), "即时单余量不得进簿");
}

#[test]
fn d1_unsupported_day_end_with_resting_orders_rolls_back_before_drain() {
    // 保留历史 exact 名称及原 30 拍输入；能力现已接通，验证真实 DayEnd 余单与守恒。
    let mut game = GameSession::new(setup(), 7).unwrap();
    let code = game.stock_id("600101").unwrap();
    game.enqueue_player_intent(AccountId(0), place(code)).unwrap();
    let mut order = None;
    for _ in 0..29 {
        for event in game.step().unwrap() {
            if let Event::OrderAccepted { id, account: AccountId(0), .. } = event { order = Some(id); }
        }
    }
    let order = order.expect("原输入必须产生真实 OrderId");
    let date = game.civil_date();
    let prefix = serde_json::to_value(game.causal_facts()).unwrap();
    let count = game.causal_facts().len();
    let events = game.step().expect("真实末拍已支持 DayEnd");
    assert!(events.iter().any(|event| matches!(event, Event::OrderCanceled { id, remaining_qty: 100, .. } if *id == order)));
    assert_eq!(serde_json::to_value(&game.causal_facts()[..count]).unwrap(), prefix);
    let terminal = game.causal_facts()[count..].iter().find(|fact| matches!(fact.kind,
        CausalFactKind::Terminated { order: id, code: actual, qty: 100, reason: Termination::DayEnd, .. } if id == order && actual == code)).unwrap();
    assert_eq!(terminal.time.market_minute, 240);
    assert_eq!(terminal.time.stock_clock.unwrap().completed_minutes, 240);
    assert_eq!(terminal.time.stock_clock.unwrap().code, code);
    assert_eq!(terminal.time.civil.date(), date);
    assert_eq!(terminal.time.civil.second_of_day(), 15 * 3600);
    let report = game.causal_diagnostics().unwrap();
    let row = report.orders.iter().find(|row| row.origin.order == order).unwrap();
    assert_eq!((row.origin.qty, row.filled_qty, row.canceled_qty, row.aborted_qty, row.open_qty), (100, 0, 100, 0, 0));
    assert_eq!(row.lifetime_market_minutes, Some(240));
    assert_eq!(report.submitted_qty, report.filled_qty + report.canceled_qty + report.aborted_qty + report.open_qty);
}
