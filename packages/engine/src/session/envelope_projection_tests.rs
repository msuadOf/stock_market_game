use super::*;
use crate::session::pipeline::ResVec;

#[test]
fn continuous_buy_and_sell_projection_carry_exact_resources_and_audit() {
    let mut game = fixture();
    let code = game.setup.stocks[0].code.clone();
    place(&mut game, &code, OrderFixture::partial_buy());
    place(&mut game, &code, OrderFixture::sell());

    let envelopes = game.project_live_envelopes().unwrap();

    assert_envelope(&envelopes[0], &code, ExpectedEnvelope::partial_buy());
    assert_envelope(&envelopes[1], &code, ExpectedEnvelope::sell());
}

#[test]
fn auction_buy_and_sell_projection_use_arrival_identity_and_zero_cumulative_audit() {
    let mut game = fixture();
    let code = game.setup.stocks[0].code.clone();
    game.auction_orders.insert(
        code.clone(),
        vec![
            AuctionOrderSnap {
                owner: AccountId(1),
                side: Side::Buy,
                limit: Money::from_cents(1_000),
                qty: 100,
                arrival_seq: 21,
            },
            AuctionOrderSnap {
                owner: AccountId(2),
                side: Side::Sell,
                limit: Money::from_cents(1_010),
                qty: 200,
                arrival_seq: 22,
            },
        ],
    );

    let envelopes = game.project_live_envelopes().unwrap();

    assert_envelope(&envelopes[0], &code, ExpectedEnvelope::auction_buy());
    assert_envelope(&envelopes[1], &code, ExpectedEnvelope::auction_sell());
}

#[test]
fn seller_projection_excludes_the_legacy_cash_reservation() {
    let mut game = fixture();
    let code = game.setup.stocks[0].code.clone();
    game.auction_orders.insert(
        code.clone(),
        vec![AuctionOrderSnap {
            owner: AccountId(1),
            side: Side::Sell,
            limit: Money::from_cents(1),
            qty: 100,
            arrival_seq: 23,
        }],
    );

    assert!(
        game.reserved_cash_for_account(AccountId(1)).unwrap() > Money::ZERO,
        "the authoritative legacy path must retain its nominal fee reservation"
    );
    let envelopes = game.project_live_envelopes().unwrap();
    assert_eq!(envelopes.len(), 1);
    assert_eq!(envelopes[0].live().cash, Money::ZERO);
    game.hydrate_or_validate_envelope_ledger().unwrap();
}

#[test]
fn live_envelope_projection_rejects_duplicate_continuous_and_auction_identity() {
    let mut game = fixture();
    let code = game.setup.stocks[0].code.clone();
    place(&mut game, &code, OrderFixture::buy(OrderId(99)));
    game.auction_orders.insert(
        code.clone(),
        vec![AuctionOrderSnap {
            owner: AccountId(1),
            side: Side::Buy,
            limit: Money::from_cents(1_000),
            qty: 100,
            arrival_seq: 99,
        }],
    );
    let before = game.envelope_ledger.clone();
    assert!(game.project_live_envelopes().is_err());
    assert!(game.hydrate_or_validate_envelope_ledger().is_err());
    assert_eq!(game.envelope_ledger, before);
}

#[test]
fn nonempty_hydration_is_idempotent_and_preserves_non_ledger_authority() {
    let mut game = fixture();
    let code = game.setup.stocks[0].code.clone();
    place(&mut game, &code, OrderFixture::buy(OrderId(31)));
    let orders = game.markets[&code].resting_orders();
    let cash = game.accounts[&AccountId(1)].cash;
    let counters = (game.tick, game.day, game.seq, game.next_receipt_base);
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let ledger = game.envelope_ledger.clone();
    game.hydrate_or_validate_envelope_ledger().unwrap();
    assert_eq!(game.envelope_ledger, ledger);
    let current_orders = game.markets[&code].resting_orders();
    assert_eq!(current_orders.len(), orders.len());
    assert_eq!(current_orders[0].id, orders[0].id);
    assert_eq!(current_orders[0].qty, orders[0].qty);
    assert_eq!(game.accounts[&AccountId(1)].cash, cash);
    assert_eq!(
        (game.tick, game.day, game.seq, game.next_receipt_base),
        counters
    );
}

fn fixture() -> GameSession {
    GameSession::new(npc_working_quote_tests::quote_setup(0), 42).unwrap()
}
struct OrderFixture {
    id: OrderId,
    side: Side,
    price: Money,
    qty: u32,
    filled_qty: u32,
    filled_value: Money,
}
impl OrderFixture {
    fn buy(id: OrderId) -> Self {
        Self {
            id,
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
        }
    }
    fn partial_buy() -> Self {
        Self {
            id: OrderId(11),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 200,
            filled_qty: 100,
            filled_value: Money::from_cents(98_000),
        }
    }
    fn sell() -> Self {
        Self {
            id: OrderId(12),
            side: Side::Sell,
            price: Money::from_cents(1_100),
            qty: 300,
            filled_qty: 0,
            filled_value: Money::ZERO,
        }
    }
}
fn place(game: &mut GameSession, code: &StockCode, fixture: OrderFixture) {
    game.markets
        .get_mut(code)
        .unwrap()
        .place(Order {
            id: fixture.id,
            side: fixture.side,
            price: fixture.price,
            qty: fixture.qty,
            original_qty: fixture.qty + fixture.filled_qty,
            filled_qty: fixture.filled_qty,
            filled_value: fixture.filled_value,
            owner: AccountId(1),
            seq: fixture.id.0,
        })
        .unwrap();
}
struct ExpectedEnvelope {
    id: OrderId,
    owner: AccountId,
    side: Side,
    resources: ResVec,
    limit: Money,
    remaining_qty: u32,
    filled_qty: u32,
    filled_value: Money,
}
impl ExpectedEnvelope {
    fn partial_buy() -> Self {
        Self {
            id: OrderId(11),
            owner: AccountId(1),
            side: Side::Buy,
            resources: ResVec::new(Money::from_cents(200_002), 0),
            limit: Money::from_cents(1_000),
            remaining_qty: 200,
            filled_qty: 100,
            filled_value: Money::from_cents(98_000),
        }
    }
    fn sell() -> Self {
        Self {
            id: OrderId(12),
            owner: AccountId(1),
            side: Side::Sell,
            resources: ResVec::new(Money::ZERO, 300),
            limit: Money::from_cents(1_100),
            remaining_qty: 300,
            filled_qty: 0,
            filled_value: Money::ZERO,
        }
    }
    fn auction_buy() -> Self {
        Self {
            id: OrderId(21),
            owner: AccountId(1),
            side: Side::Buy,
            resources: ResVec::new(Money::from_cents(100_501), 0),
            limit: Money::from_cents(1_000),
            remaining_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
        }
    }
    fn auction_sell() -> Self {
        Self {
            id: OrderId(22),
            owner: AccountId(2),
            side: Side::Sell,
            resources: ResVec::new(Money::ZERO, 200),
            limit: Money::from_cents(1_010),
            remaining_qty: 200,
            filled_qty: 0,
            filled_value: Money::ZERO,
        }
    }
}
fn assert_envelope(envelope: &pipeline::Envelope, code: &StockCode, expected: ExpectedEnvelope) {
    assert_eq!(
        envelope.key(),
        &pipeline::EnvelopeKey {
            account: expected.owner,
            stock: code.clone(),
            order: expected.id,
            side: expected.side
        }
    );
    assert_eq!(envelope.origin(), pipeline::EnvelopeOrigin::TickStart);
    assert_eq!(envelope.basis(), expected.resources);
    assert_eq!(envelope.live(), expected.resources);
    assert_eq!(envelope.spent(), pipeline::ResVec::ZERO);
    assert_eq!(envelope.released(), pipeline::ResVec::ZERO);
    assert_eq!(
        envelope.audit(),
        pipeline::EnvelopeAudit {
            limit: expected.limit,
            remaining_qty: expected.remaining_qty,
            filled_qty: expected.filled_qty,
            filled_value: expected.filled_value,
            nominal: pipeline::FeeComponents::ZERO,
            charged: pipeline::FeeComponents::ZERO
        }
    );
}
