use super::*;
use crate::session::pipeline::{
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, FeeComponents, ResVec,
};

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

#[test]
fn hydration_rejects_nominal_fee_drift_without_mutating_the_ledger() {
    let mut game = fixture();
    let code = game.setup.stocks[0].code.clone();
    game.markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: OrderId(51),
            side: Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
            original_qty: 200,
            filled_qty: 100,
            filled_value: Money::from_cents(98_000),
            owner: AccountId(1),
            seq: 51,
        })
        .unwrap();
    let projected = game.project_live_envelopes().unwrap().remove(0);
    let mut audit = projected.audit();
    audit.nominal.commission = audit.nominal.commission.add(Money::from_cents(1)).unwrap();
    audit.charged = audit.nominal;
    game.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            projected.key().clone(),
            projected.live().cash,
            projected.live().shares,
            audit,
        )],
    )
    .unwrap();
    let before = game.envelope_ledger.clone();

    assert!(game.hydrate_or_validate_envelope_ledger().is_err());
    assert_eq!(game.envelope_ledger, before);
}

#[test]
fn hydration_preserves_valid_seller_debt_and_rejects_out_of_bounds_charge() {
    let mut game = fixture();
    let code = game.setup.stocks[0].code.clone();
    game.markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: OrderId(52),
            side: Side::Sell,
            price: Money::from_cents(1_000),
            qty: 100,
            original_qty: 101,
            filled_qty: 1,
            filled_value: Money::from_cents(100),
            owner: AccountId(1),
            seq: 52,
        })
        .unwrap();
    let projected = game.project_live_envelopes().unwrap().remove(0);
    let valid = EnvelopeAudit {
        charged: FeeComponents {
            commission: Money::from_cents(100),
            stamp_tax: Money::ZERO,
            transfer_fee: Money::ZERO,
        },
        ..projected.audit()
    };
    assert_eq!(valid.charged.commission, Money::from_cents(100));
    assert_eq!(valid.charged.stamp_tax, Money::ZERO);
    assert!(valid.charged.total().unwrap() < valid.nominal.total().unwrap());
    game.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            projected.key().clone(),
            projected.live().cash,
            projected.live().shares,
            valid,
        )],
    )
    .unwrap();

    game.hydrate_or_validate_envelope_ledger().unwrap();
    assert_eq!(
        game.envelope_ledger.get(projected.key()).unwrap().audit(),
        valid
    );

    let out_of_bounds = EnvelopeAudit {
        charged: FeeComponents {
            commission: Money::from_cents(101),
            stamp_tax: Money::ZERO,
            transfer_fee: Money::ZERO,
        },
        ..valid
    };
    game.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            projected.key().clone(),
            projected.live().cash,
            projected.live().shares,
            out_of_bounds,
        )],
    )
    .unwrap();
    let before = game.envelope_ledger.clone();

    assert!(game.hydrate_or_validate_envelope_ledger().is_err());
    assert_eq!(game.envelope_ledger, before);
}

#[test]
fn hydration_preserves_path_dependent_rounding_debt() {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.stocks[0].initial_price = Money::from_cents(1);
    setup.config.commission_min = Money::ZERO;
    let mut game = GameSession::new(setup, 42).unwrap();
    let code = game.setup.stocks[0].code.clone();
    game.markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: OrderId(53),
            side: Side::Sell,
            price: Money::from_cents(1),
            qty: 1,
            original_qty: 50_002,
            filled_qty: 50_001,
            filled_value: Money::from_cents(50_001),
            owner: AccountId(1),
            seq: 53,
        })
        .unwrap();
    let projected = game.project_live_envelopes().unwrap().remove(0);
    let path_dependent = EnvelopeAudit {
        charged: FeeComponents {
            commission: Money::from_cents(13),
            stamp_tax: Money::from_cents(25),
            transfer_fee: Money::ZERO,
        },
        ..projected.audit()
    };
    assert_eq!(path_dependent.nominal.commission, Money::from_cents(13));
    assert_eq!(path_dependent.nominal.stamp_tax, Money::from_cents(25));
    assert_eq!(path_dependent.nominal.transfer_fee, Money::from_cents(1));
    game.envelope_ledger = EnvelopeLedger::new(
        0,
        [Envelope::tick_start_existing(
            projected.key().clone(),
            projected.live().cash,
            projected.live().shares,
            path_dependent,
        )],
    )
    .unwrap();

    game.hydrate_or_validate_envelope_ledger().unwrap();
    assert_eq!(
        game.envelope_ledger.get(projected.key()).unwrap().audit(),
        path_dependent
    );
}

#[test]
fn hydration_rejects_partial_seller_when_charged_history_is_missing() {
    let mut game = fixture();
    let code = game.setup.stocks[0].code.clone();
    game.markets
        .get_mut(&code)
        .unwrap()
        .place(Order {
            id: OrderId(54),
            side: Side::Sell,
            price: Money::from_cents(1_000),
            qty: 100,
            original_qty: 101,
            filled_qty: 1,
            filled_value: Money::from_cents(100),
            owner: AccountId(1),
            seq: 54,
        })
        .unwrap();
    let before = game.envelope_ledger.clone();

    let error = game.hydrate_or_validate_envelope_ledger().unwrap_err();
    assert!(error.to_string().contains("charged fee history"));
    assert_eq!(game.envelope_ledger, before);
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
