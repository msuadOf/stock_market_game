use crate::{frames, scenarios::setup, CorpusError};
use engine::{
    AccountId, GameSession, Intent, Money, Order, OrderId, PositionSnap, Side, StockCode,
};
use serde_json::{json, Value};

pub fn collect(seed: u64) -> Result<Value, CorpusError> {
    let mut config = setup(1)?;
    config.npcs.retail_count = 1;
    config.npcs.retail_cash_median = Money::from_cents(1_000_000);
    config.stocks[0].float_shares = 2_400;
    let mut initial = GameSession::new(config, seed)?.save()?;
    let code = StockCode("600001".to_owned());
    for attention in initial.npc_attention.values_mut() {
        attention.next_attention_candidate_tick = 100;
    }
    let mut session = GameSession::restore(&initial)?;
    for _ in 0..4 {
        session.step()?;
    }
    let mut save = session.save()?;
    let seller = save
        .snapshot
        .accounts
        .get_mut(&AccountId(1))
        .ok_or_else(|| CorpusError::Invariant("seller missing".to_owned()))?;
    seller.positions.insert(
        code.clone(),
        PositionSnap {
            qty: 2_400,
            t1_locked: 0,
            invested_cents: 2_400,
            recovered_cents: 0,
        },
    );
    save.resting_orders.insert(
        code.clone(),
        vec![Order {
            id: OrderId(1),
            owner: AccountId(1),
            side: Side::Sell,
            price: Money::from_cents(1),
            qty: 1_200,
            original_qty: 1_200,
            filled_qty: 0,
            filled_value: Money::ZERO,
            seq: 1,
        }],
    );
    let market = save
        .snapshot
        .markets
        .get_mut(&code)
        .ok_or_else(|| CorpusError::Invariant("market missing".to_owned()))?;
    market.best_ask = Some(Money::from_cents(1));
    market.asks = vec![(Money::from_cents(1), 1_200)];
    save.next_order_id = 2;
    session = GameSession::restore(&save)?;
    let mut legs = Vec::new();
    for (quantity, expected_net) in [(100, -400), (100, 100), (1_000, 999)] {
        let before = session.save()?;
        session.enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: Money::from_cents(1),
                qty: quantity,
            },
        )?;
        let frame = frames::step(&mut session)?;
        let after = session.save()?;
        let before_seller = before
            .snapshot
            .accounts
            .get(&AccountId(1))
            .ok_or_else(|| CorpusError::Invariant("seller absent".to_owned()))?;
        let after_seller = after
            .snapshot
            .accounts
            .get(&AccountId(1))
            .ok_or_else(|| CorpusError::Invariant("seller absent".to_owned()))?;
        let actual_net = after_seller.cash.cents() - before_seller.cash.cents();
        if actual_net != expected_net {
            return Err(CorpusError::Invariant(format!(
                "independent seller net {actual_net} != {expected_net}"
            )));
        }
        let before_position = before_seller
            .positions
            .get(&code)
            .ok_or_else(|| CorpusError::Invariant("seller position absent".to_owned()))?;
        let after_position = after_seller
            .positions
            .get(&code)
            .ok_or_else(|| CorpusError::Invariant("seller position absent".to_owned()))?;
        if before_position.qty - after_position.qty != quantity || after_position.t1_locked != 0 {
            return Err(CorpusError::Invariant(
                "seller share conservation/T+1 mismatch".to_owned(),
            ));
        }
        legs.push(json!({"frame":frame,"seller_before":before_seller,"seller_after":after_seller,
            "gross_cents":quantity,"observed_net_cents":actual_net,"observed_fee_cents":i64::from(quantity)-actual_net}));
    }
    Ok(
        json!({"kind":"independent_seller_fee_control","seller":1,"buyer":0,
        "npc_attention_disabled_until_tick":100,"initial_state":save,"legs":legs}),
    )
}
