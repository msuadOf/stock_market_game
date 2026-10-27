//! feature on/off 共用固定操作合同：业务事实、资源与保存恢复不依赖诊断开关。

mod common;
use engine::{AccountId, Event, GameSession, Intent, LimitPrice, Money, Side};

#[test]
fn fixed_player_operations_preserve_transaction_resources_events_and_restore() {
    let mut setup = common::flip_helpers::setup_1stock();
    setup.npcs.retail_count = 0;
    setup.ticks_per_day = 30;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    let mut game = GameSession::new(setup, 7).unwrap();
    let code = game.stock_id("600200").unwrap();
    let cash = game.account(AccountId(0)).unwrap().cash();
    game.enqueue_player_intent(AccountId(0), Intent::PlaceLimit {
        code, side: Side::Buy, price: LimitPrice::Fixed(Money::from_cents(995)), qty: 100,
    }).unwrap();
    let first = game.step().unwrap();
    let order = first.iter().find_map(|event| match event {
        Event::OrderAccepted { id, remaining_qty: 100, .. } => Some(*id), _ => None,
    }).expect("首拍真实受理100股");
    assert!(!first.iter().any(|event| matches!(event, Event::Trade { .. })));
    game.enqueue_player_intent(AccountId(0), Intent::Cancel { code, id: order }).unwrap();
    let second = game.step().unwrap();
    assert!(second.iter().any(|event| matches!(event, Event::OrderCanceled { id, remaining_qty: 100, .. } if *id == order)));
    assert!(!second.iter().any(|event| matches!(event, Event::Trade { .. })));
    assert_eq!(game.account(AccountId(0)).unwrap().cash(), cash);
    let saved = game.save().unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    assert_eq!(serde_json::to_value(restored.save().unwrap()).unwrap(), serde_json::to_value(saved).unwrap());
}
