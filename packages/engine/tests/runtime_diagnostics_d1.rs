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
    let mut session = ProtocolSession::new(setup(), 7).unwrap();
    let code = session.game().stock_id("600101").unwrap();
    session
        .enqueue_player_intent(AccountId(0), place(code))
        .unwrap();
    let first = session.step_frame().unwrap();
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
    session.step_frame().unwrap();
    let second_checkpoint = session.checkpoint().unwrap();
    let second_facts = serde_json::to_value(session.game().causal_facts()).unwrap();
    session.step_frame().unwrap();
    session.step_frame().unwrap();
    session.rollback(first_checkpoint);
    assert_eq!(
        serde_json::to_value(session.game().causal_facts()).unwrap(),
        first_facts
    );
    assert_eq!(session.game().business_state_hash().unwrap(), first_hash);
    let mut other = ProtocolSession::new(setup(), 99).unwrap();
    other.rollback(second_checkpoint);
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
    other.step_frame().unwrap();
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
    let mut game = GameSession::new(setup(), 7).unwrap();
    let code = game.stock_id("600101").unwrap();
    game.enqueue_player_intent(AccountId(0), Intent::PlaceMarket { code, side: Side::Buy, qty: 100 }).unwrap();
    let before = game.business_state_hash().unwrap();
    let outcome = game.step();
    assert!(matches!(outcome, Err(engine::session::StepFatal::InvariantViolation { ref location, ref description })
        if location == "runtime::diagnostics::prepare" && description.contains("Market")),
        "未接通的Market余量终止不能报成open订单：{outcome:?}");
    assert!(game.causal_facts().is_empty());
    assert_eq!(game.business_state_hash().unwrap(), before);
}

#[test]
fn d1_unsupported_day_end_with_resting_orders_rolls_back_before_drain() {
    let mut game = GameSession::new(setup(), 7).unwrap();
    let code = game.stock_id("600101").unwrap();
    game.enqueue_player_intent(AccountId(0), place(code)).unwrap();
    for _ in 0..29 { game.step().unwrap(); }
    let before = game.business_state_hash().unwrap();
    let facts = serde_json::to_value(game.causal_facts()).unwrap();
    let outcome = game.step();
    assert!(matches!(outcome, Err(engine::session::StepFatal::InvariantViolation { ref location, ref description })
        if location == "runtime::diagnostics::prepare" && description.contains("DayEnd")),
        "未接通的DayEnd终止不能仍报open订单：{outcome:?}");
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(serde_json::to_value(game.causal_facts()).unwrap(), facts);
}
