use super::{
    conservation::{ConservationBasis, ConservationRow},
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeOrigin, FeeComponents, ReceiptDelta, ResVec,
};
use crate::{AccountId, Money, OrderId, Side, StockCode};

#[test]
fn conservation_existing_envelope_when_fill_and_release_match_live() {
    let mut envelope =
        Envelope::tick_start_existing(key(Side::Buy), Money::from_cents(1_005), 0, audit(0));
    envelope
        .apply(
            ReceiptDelta::sealed(
                ResVec::new(Money::from_cents(905), 0),
                ResVec::new(Money::from_cents(100), 0),
                ResVec::ZERO,
            ),
            audit(0),
        )
        .unwrap();
    assert_eq!(envelope.live(), ResVec::ZERO);
}

#[test]
fn conservation_negative_release_is_fatal() {
    let mut envelope = Envelope::tick_start_existing(key(Side::Sell), Money::ZERO, 10, audit(10));
    assert!(envelope
        .apply(
            ReceiptDelta::sealed(ResVec::ZERO, ResVec::new(Money::ZERO, 11), ResVec::ZERO),
            audit(0)
        )
        .is_err());
}

#[test]
fn conservation_negative_envelope_basis_is_fatal() {
    let envelope =
        Envelope::tick_start_existing(key(Side::Buy), Money::from_cents(-1), 0, audit(0));

    assert!(envelope.validate().is_err());
}

#[test]
fn p3_created_envelope_exposes_created_origin_and_basis() {
    let envelope = Envelope::p3_created(key(Side::Buy), Money::from_cents(100), 0, audit(0));
    assert_eq!(envelope.origin(), EnvelopeOrigin::P3Created);
    assert_eq!(envelope.basis(), ResVec::new(Money::from_cents(100), 0));
}

#[test]
fn conservation_row_existing_envelope_proves_preseal_and_sealed_equations() {
    let row = ConservationRow {
        key: key(Side::Buy),
        basis: ConservationBasis::TickStart(
            ResVec::new(Money::from_cents(100), 10),
            ResVec::new(Money::from_cents(40), 4),
            ResVec::new(Money::from_cents(60), 6),
        ),
        sealed_spent: ResVec::new(Money::from_cents(50), 5),
        sealed_released: ResVec::new(Money::from_cents(10), 1),
        commit_live: ResVec::ZERO,
    };

    row.validate().unwrap();
}

#[test]
fn conservation_cross_resource_leak_is_fatal_per_key() {
    let row = ConservationRow {
        key: key(Side::Buy),
        basis: ConservationBasis::TickStart(
            ResVec::new(Money::from_cents(100), 10),
            ResVec::new(Money::ZERO, 10),
            ResVec::new(Money::from_cents(100), 1),
        ),
        sealed_spent: ResVec::ZERO,
        sealed_released: ResVec::ZERO,
        commit_live: ResVec::new(Money::from_cents(100), 1),
    };

    assert!(row.validate().is_err());
}

#[test]
fn conservation_negative_release_is_fatal_in_a_row() {
    let row = ConservationRow {
        key: key(Side::Buy),
        basis: ConservationBasis::P3Created(ResVec::new(Money::from_cents(100), 0)),
        sealed_spent: ResVec::ZERO,
        sealed_released: ResVec::new(Money::from_cents(-1), 0),
        commit_live: ResVec::new(Money::from_cents(101), 0),
    };

    assert!(row.validate().is_err());
}

#[test]
fn conservation_created_envelope_rejects_a_p0_contribution() {
    let row = ConservationRow {
        key: key(Side::Buy),
        basis: ConservationBasis::P3Created(ResVec::new(Money::from_cents(100), 0)),
        sealed_spent: ResVec::ZERO,
        sealed_released: ResVec::ZERO,
        commit_live: ResVec::new(Money::from_cents(100), 0),
    };

    assert!(row
        .validate_with_preseal(ResVec::new(Money::from_cents(1), 0))
        .is_err());
}

#[test]
fn conservation_row_rejects_a_per_key_mismatch() {
    let row = ConservationRow {
        key: key(Side::Buy),
        basis: ConservationBasis::P3Created(ResVec::new(Money::from_cents(100), 0)),
        sealed_spent: ResVec::ZERO,
        sealed_released: ResVec::ZERO,
        commit_live: ResVec::new(Money::from_cents(99), 0),
    };

    assert!(row.validate().is_err());
}

fn key(side: Side) -> EnvelopeKey {
    EnvelopeKey {
        account: AccountId(1),
        stock: StockCode("600001".to_owned()),
        order: OrderId(1),
        side,
    }
}
fn audit(remaining_qty: u32) -> EnvelopeAudit {
    EnvelopeAudit {
        limit: Money::ZERO,
        remaining_qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
    }
}
