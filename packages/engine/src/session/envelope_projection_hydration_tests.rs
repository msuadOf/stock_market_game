use super::*;
use crate::session::pipeline::{EnvelopeKey, ResVec};

#[test]
fn nonempty_projection_is_pure_and_later_live_key_mismatch_preserves_hydrated_ledger() {
    let mut game = fixture();
    let code = game.setup.stocks[0].code.clone();
    place(&mut game, &code, OrderId(41), 100);
    game.auction_orders.insert(
        code.clone(),
        vec![AuctionOrderSnap {
            owner: AccountId(2),
            side: Side::Sell,
            limit: Money::from_cents(1_100),
            qty: 100,
            arrival_seq: 42,
        }],
    );
    let before = game.business_state_hash().unwrap();

    let projected = game.project_live_envelopes().unwrap();

    assert_eq!(projected.len(), 2);
    assert_envelope(
        &projected[0],
        EnvelopeKey {
            account: AccountId(1),
            stock: code.clone(),
            order: OrderId(41),
            side: Side::Buy,
        },
        ResVec::new(Money::from_cents(100_501), 0),
        Money::from_cents(1_000),
    );
    assert_envelope(
        &projected[1],
        EnvelopeKey {
            account: AccountId(2),
            stock: code.clone(),
            order: OrderId(42),
            side: Side::Sell,
        },
        ResVec::new(Money::ZERO, 100),
        Money::from_cents(1_100),
    );
    assert_eq!(game.business_state_hash().unwrap(), before);
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let ledger = game.envelope_ledger.clone();
    game.markets
        .get_mut(&code)
        .unwrap()
        .cancel(OrderId(41))
        .unwrap();
    place(&mut game, &code, OrderId(41), 200);

    assert!(game.hydrate_or_validate_envelope_ledger().is_err());
    assert_eq!(game.envelope_ledger, ledger);
    assert_eq!(game.envelope_ledger.next_receipt_index(), 0);
}

fn fixture() -> GameSession {
    GameSession::new(npc_working_quote_tests::quote_setup(0), 42).unwrap()
}

fn place(game: &mut GameSession, code: &StockCode, id: OrderId, qty: u32) {
    game.markets
        .get_mut(code)
        .unwrap()
        .place(Order {
            id,
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty,
            original_qty: qty,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: AccountId(1),
            seq: id.0,
        })
        .unwrap();
}

fn assert_envelope(
    envelope: &pipeline::Envelope,
    key: EnvelopeKey,
    resources: ResVec,
    limit: Money,
) {
    assert_eq!(envelope.key(), &key);
    assert_eq!(envelope.origin(), pipeline::EnvelopeOrigin::TickStart);
    assert_eq!(envelope.basis(), resources);
    assert_eq!(envelope.live(), resources);
    assert_eq!(envelope.spent(), ResVec::ZERO);
    assert_eq!(envelope.released(), ResVec::ZERO);
    assert_eq!(
        envelope.audit(),
        pipeline::EnvelopeAudit {
            limit,
            remaining_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            nominal: pipeline::FeeComponents::ZERO,
            charged: pipeline::FeeComponents::ZERO,
        }
    );
}
