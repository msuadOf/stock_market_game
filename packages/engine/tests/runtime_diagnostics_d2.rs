#![cfg(feature = "simulation-diagnostics")]

#[path = "diagnostic_parity.rs"]
mod fixture;

use engine::{AccountId, GameSession, Intent, LimitPrice, Money, Side, StockId};
use engine::diagnostics::causal::CausalFactKind;

#[test]
fn d2_real_ordinary_players_publish_exact_continuous_fill() {
    let mut setup = fixture::setup();
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    let mut game = GameSession::new(setup, 7).unwrap();
    let seller = game.join_market(engine::session::OpaqueSubjectId::new("d2-seller".to_owned()).unwrap(), true).unwrap().account_id;
    let mut save = game.save().unwrap();
    save.snapshot.accounts.get_mut(&seller).unwrap().positions.insert(StockId(0), engine::session::PositionSnap {
        qty: 100, t1_locked: 0, invested_cents: 100_000, recovered_cents: 0,
    });
    let mut game = GameSession::restore(&save).unwrap();
    for (account, side) in [(seller, Side::Sell), (AccountId(0), Side::Buy)] {
        game.enqueue_player_intent(account, Intent::PlaceLimit {
            code: StockId(0), side, price: LimitPrice::Fixed(Money::from_cents(995)), qty: 100,
        }).unwrap();
    }
    let events = game.step().expect("真实 ordinary player 连续成交必须成功");
    let filled = game.causal_facts().iter().filter_map(|fact| match fact.kind {
        CausalFactKind::Filled { account, code, qty, value_before, gross, .. } =>
            Some((account, code, qty, value_before, gross)),
        _ => None,
    }).collect::<Vec<_>>();
    assert_eq!(filled, vec![(seller, StockId(0), 100, 0, 99_500), (AccountId(0), StockId(0), 100, 0, 99_500)]);
    assert_eq!(events.iter().filter(|event| matches!(event, engine::Event::Trade { qty: 100, .. })).count(), 1);
    assert_eq!(game.causal_facts().iter().filter(|fact| matches!(fact.kind, CausalFactKind::Execution { qty: 100, price_cents: 995, .. })).count(), 1);
}
