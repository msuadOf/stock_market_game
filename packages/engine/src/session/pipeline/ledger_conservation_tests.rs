use super::ledger_tests::*;
use super::{Envelope, EnvelopeLedger, ReceiptDelta, ResVec};
use crate::Money;

#[test]
fn conservation_aggregate_sums_the_same_cash_and_share_equations_per_key() {
    let buy = key();
    let sell = second_key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [
            Envelope::tick_start_existing(
                buy.clone(),
                Money::from_cents(100),
                0,
                audit_with_remaining(100),
            ),
            Envelope::p3_created(sell.clone(), Money::ZERO, 100, audit_with_remaining(100)),
        ],
    )
    .unwrap();
    let mut receipts = [
        sealed_receipt(
            buy,
            ReceiptDelta::sealed(
                ResVec::new(Money::from_cents(90), 0),
                ResVec::new(Money::from_cents(10), 0),
                ResVec::ZERO,
            ),
            0,
        ),
        sealed_receipt(
            sell,
            ReceiptDelta::sealed(ResVec::new(Money::ZERO, 100), ResVec::ZERO, ResVec::ZERO),
            0,
        ),
    ];
    receipts[0].value_after = Money::from_cents(90);
    ledger.apply(&mut receipts).unwrap();
    ledger.validate_conservation().unwrap();
}
#[test]
fn conservation_aggregate_rejects_cross_key_resource_leaks_without_mutation() {
    let first = Envelope::tick_start_existing(key(), Money::from_cents(100), 0, audit());
    let second = Envelope::tick_start_existing(second_key(), Money::ZERO, 100, audit());
    let mut ledger = EnvelopeLedger::new(7, [first, second]).unwrap();
    let before = ledger.clone();
    let mut receipts = [
        sealed_receipt(
            key(),
            ReceiptDelta::sealed(
                ResVec::new(Money::from_cents(100), 0),
                ResVec::ZERO,
                ResVec::ZERO,
            ),
            0,
        ),
        sealed_receipt(
            second_key(),
            ReceiptDelta::sealed(ResVec::new(Money::ZERO, 99), ResVec::ZERO, ResVec::ZERO),
            1,
        ),
    ];
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
}
#[test]
fn conservation_apply_rechecks_the_aggregate_before_installing_receipts() {
    let mut ledger = created_ledger();
    let before = ledger.clone();
    let mut receipts = [sealed_receipt(
        key(),
        ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(99), 0),
            ResVec::ZERO,
            ResVec::ZERO,
        ),
        0,
    )];
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
}
