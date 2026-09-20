use super::ledger_tests::*;
use super::{
    Envelope, EnvelopeLedger, FeeComponents, ReceiptDelta, ReceiptKind, ReceiptSource, ResVec,
};
use crate::Money;

#[test]
fn conservation_duplicate_receipt_is_fatal_without_mutation() {
    let mut ledger = created_ledger();
    let before = ledger.clone();
    let mut receipts = [receipt(0), receipt(0)];
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
    assert_eq!(ledger.next_receipt_index(), 7);
}
#[test]
fn conservation_receipt_indices_are_global() {
    let mut ledger = created_ledger();
    let mut receipts = [receipt(0)];
    ledger.apply(&mut receipts).unwrap();
    assert_eq!(receipts[0].index, 7);
    assert_eq!(ledger.next_receipt_index(), 8);
}

#[test]
fn successful_fill_updates_the_envelope_and_ledger_audit_together() {
    let mut ledger = created_ledger();
    let mut receipts = [receipt(0)];
    receipts[0].deliver_qty = 100;

    ledger.apply(&mut receipts).unwrap();

    let audit = ledger.get(&receipts[0].envelope).unwrap().audit();
    assert_eq!(audit.remaining_qty, 0);
    assert_eq!(audit.filled_qty, 100);
    assert_eq!(audit.filled_value, Money::from_cents(100));
    assert_eq!(audit.nominal, receipts[0].nominal);
    assert_eq!(audit.charged, receipts[0].charged_after);
}
#[test]
fn receipt_before_fields_must_match_the_envelope_audit_before_mutation() {
    let mut ledger = created_ledger();
    let before = ledger.clone();
    let mut receipts = [receipt(0)];
    receipts[0].qty_before = 1;
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
}
#[test]
fn rejected_apply_does_not_reorder_or_index_the_caller_receipts() {
    let mut ledger = created_ledger();
    let mut receipts = [receipt(1), receipt(0)];
    let before = receipts.clone();
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(receipts[0].index, before[0].index);
    assert_eq!(receipts[1].index, before[1].index);
    assert_eq!(receipts[0].local_key, before[0].local_key);
    assert_eq!(receipts[1].local_key, before[1].local_key);
}
#[test]
fn seller_cash_or_spent_resources_are_rejected_transactionally() {
    let key = second_key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key.clone(),
            Money::ZERO,
            100,
            audit_with_remaining(100),
        )],
    )
    .unwrap();
    let before = ledger.clone();
    let mut receipts = [sealed_receipt(
        key,
        ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(1), 100),
            ResVec::ZERO,
            ResVec::ZERO,
        ),
        0,
    )];
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
}
#[test]
fn seller_fill_delivery_cash_must_equal_gross_less_charged() {
    let key = second_key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key.clone(),
            Money::ZERO,
            100,
            audit_with_remaining(100),
        )],
    )
    .unwrap();
    let before = ledger.clone();
    let mut receipts = [sealed_receipt(
        key,
        ReceiptDelta::sealed(ResVec::new(Money::ZERO, 100), ResVec::ZERO, ResVec::ZERO),
        0,
    )];
    receipts[0].deliver_cash = Money::from_cents(-1);
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
}

#[test]
fn non_fill_receipts_cannot_spend_or_accrue_fees() {
    let envelope_key = key();
    let envelope = || {
        Envelope::p3_created(
            envelope_key.clone(),
            Money::from_cents(100),
            0,
            audit_with_remaining(100),
        )
    };

    let mut release_ledger = EnvelopeLedger::new(7, [envelope()]).unwrap();
    let release_before = release_ledger.clone();
    let mut release = receipt_from_source(ReceiptFixture {
        key: envelope_key.clone(),
        source: ReceiptSource::SealedIntent(0),
        kind: ReceiptKind::Release,
        qty_before: 100,
        qty_after: 100,
        value_before: Money::ZERO,
        value_after: Money::ZERO,
        delta: ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(1), 0),
            ResVec::new(Money::from_cents(99), 0),
            ResVec::ZERO,
        ),
    });
    assert!(release_ledger
        .apply(std::slice::from_mut(&mut release))
        .is_err());
    assert_eq!(release_ledger, release_before);

    let mut reject_ledger = EnvelopeLedger::new(7, [envelope()]).unwrap();
    let reject_before = reject_ledger.clone();
    let mut reject = receipt_from_source(ReceiptFixture {
        key: envelope_key,
        source: ReceiptSource::SealedIntent(0),
        kind: ReceiptKind::Reject,
        qty_before: 100,
        qty_after: 100,
        value_before: Money::ZERO,
        value_after: Money::ZERO,
        delta: ReceiptDelta::sealed(
            ResVec::ZERO,
            ResVec::new(Money::from_cents(100), 0),
            ResVec::ZERO,
        ),
    });
    reject.nominal.commission = Money::from_cents(1);
    assert!(reject_ledger
        .apply(std::slice::from_mut(&mut reject))
        .is_err());
    assert_eq!(reject_ledger, reject_before);

    let mut charged_audit = audit_with_remaining(100);
    charged_audit.nominal.commission = Money::from_cents(1);
    let mut charged_ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key(),
            Money::from_cents(100),
            0,
            charged_audit,
        )],
    )
    .unwrap();
    let charged_before = charged_ledger.clone();
    let mut charged_reject = receipt_from_source(ReceiptFixture {
        key: key(),
        source: ReceiptSource::SealedIntent(0),
        kind: ReceiptKind::Reject,
        qty_before: 100,
        qty_after: 100,
        value_before: Money::ZERO,
        value_after: Money::ZERO,
        delta: ReceiptDelta::sealed(
            ResVec::ZERO,
            ResVec::new(Money::from_cents(100), 0),
            ResVec::ZERO,
        ),
    });
    charged_reject.charged.commission = Money::from_cents(1);
    charged_reject.charged_after.commission = Money::from_cents(1);
    assert!(charged_ledger
        .apply(std::slice::from_mut(&mut charged_reject))
        .is_err());
    assert_eq!(charged_ledger, charged_before);
}

#[test]
fn release_and_reject_may_release_but_never_spend_live_resources() {
    for kind in [ReceiptKind::Release, ReceiptKind::Reject] {
        let key = key();
        let mut ledger = EnvelopeLedger::new(
            7,
            [Envelope::p3_created(
                key.clone(),
                Money::from_cents(100),
                0,
                audit_with_remaining(100),
            )],
        )
        .unwrap();
        let mut receipt = receipt_from_source(ReceiptFixture {
            key,
            source: ReceiptSource::SealedIntent(0),
            kind,
            qty_before: 100,
            qty_after: 100,
            value_before: Money::ZERO,
            value_after: Money::ZERO,
            delta: ReceiptDelta::sealed(
                ResVec::ZERO,
                ResVec::new(Money::from_cents(100), 0),
                ResVec::ZERO,
            ),
        });

        ledger.apply(std::slice::from_mut(&mut receipt)).unwrap();
    }
}

#[test]
fn seller_rollover_keeps_every_unfilled_share_live() {
    let key = second_key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key.clone(),
            Money::ZERO,
            100,
            audit_with_remaining(100),
        )],
    )
    .unwrap();
    let before = ledger.clone();
    let mut rollover = receipt_from_source(ReceiptFixture {
        key,
        source: ReceiptSource::Auction(0),
        kind: ReceiptKind::Rollover,
        qty_before: 100,
        qty_after: 100,
        value_before: Money::ZERO,
        value_after: Money::ZERO,
        delta: ReceiptDelta::sealed(
            ResVec::ZERO,
            ResVec::new(Money::ZERO, 1),
            ResVec::new(Money::ZERO, 99),
        ),
    });

    assert!(ledger.apply(std::slice::from_mut(&mut rollover)).is_err());
    assert_eq!(ledger, before);

    let key = second_key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key.clone(),
            Money::ZERO,
            100,
            audit_with_remaining(100),
        )],
    )
    .unwrap();
    let mut valid = receipt_from_source(ReceiptFixture {
        key,
        source: ReceiptSource::Auction(0),
        kind: ReceiptKind::Rollover,
        qty_before: 100,
        qty_after: 100,
        value_before: Money::ZERO,
        value_after: Money::ZERO,
        delta: ReceiptDelta::sealed(ResVec::ZERO, ResVec::ZERO, ResVec::new(Money::ZERO, 100)),
    });
    ledger.apply(std::slice::from_mut(&mut valid)).unwrap();
}

#[test]
fn seller_fill_requires_the_full_capped_charge_delta() {
    let key = second_key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key.clone(),
            Money::ZERO,
            100,
            audit_with_remaining(100),
        )],
    )
    .unwrap();
    let before = ledger.clone();
    let mut receipt = sealed_receipt(
        key,
        ReceiptDelta::sealed(ResVec::new(Money::ZERO, 100), ResVec::ZERO, ResVec::ZERO),
        0,
    );
    receipt.nominal.commission = Money::from_cents(500);

    assert!(ledger.apply(std::slice::from_mut(&mut receipt)).is_err());
    assert_eq!(ledger, before);
}

#[test]
fn seller_fill_charges_unpaid_components_in_fixed_priority_order() {
    let key = second_key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key.clone(),
            Money::ZERO,
            100,
            audit_with_remaining(100),
        )],
    )
    .unwrap();
    let before = ledger.clone();
    let mut receipt = sealed_receipt(
        key,
        ReceiptDelta::sealed(ResVec::new(Money::ZERO, 100), ResVec::ZERO, ResVec::ZERO),
        0,
    );
    receipt.nominal = FeeComponents {
        commission: Money::from_cents(100),
        stamp_tax: Money::from_cents(100),
        transfer_fee: Money::from_cents(100),
    };
    receipt.charged = FeeComponents {
        commission: Money::ZERO,
        stamp_tax: Money::from_cents(100),
        transfer_fee: Money::ZERO,
    };
    receipt.charged_after = receipt.charged;
    receipt.deliver_cash = Money::ZERO;

    assert!(ledger.apply(std::slice::from_mut(&mut receipt)).is_err());
    assert_eq!(ledger, before);
}

#[test]
fn seller_fill_rejects_charged_history_above_cumulative_gross() {
    let key = second_key();
    let mut prior = audit_with_remaining(100);
    prior.filled_qty = 1;
    prior.filled_value = Money::from_cents(100);
    prior.nominal.commission = Money::from_cents(500);
    prior.charged.commission = Money::from_cents(101);
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(key.clone(), Money::ZERO, 100, prior)],
    )
    .unwrap();
    let before = ledger.clone();
    let mut receipt = sealed_receipt(
        key,
        ReceiptDelta::sealed(ResVec::new(Money::ZERO, 100), ResVec::ZERO, ResVec::ZERO),
        0,
    );
    receipt.value_before = Money::from_cents(100);
    receipt.value_after = Money::from_cents(200);
    receipt.charged_before = prior.charged;
    receipt.charged.commission = Money::from_cents(100);
    receipt.charged_after.commission = Money::from_cents(201);
    receipt.deliver_cash = Money::ZERO;

    assert!(ledger.apply(std::slice::from_mut(&mut receipt)).is_err());
    assert_eq!(ledger, before);
}

#[test]
fn buyer_fill_delivers_shares_and_spends_exactly_gross_plus_charged() {
    let mut ledger = created_ledger();
    let mut valid = receipt(0);
    valid.deliver_qty = 100;
    ledger.apply(std::slice::from_mut(&mut valid)).unwrap();

    let mut ledger = created_ledger();
    let mut invalid = receipt(0);
    invalid.deliver_qty = 100;
    invalid.delta = ReceiptDelta::sealed(
        ResVec::new(Money::from_cents(99), 0),
        ResVec::new(Money::from_cents(1), 0),
        ResVec::ZERO,
    );
    assert!(ledger.apply(std::slice::from_mut(&mut invalid)).is_err());
}

#[test]
fn seller_fill_spends_exact_fill_shares_and_delivers_only_cash() {
    let key = second_key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key.clone(),
            Money::ZERO,
            100,
            audit_with_remaining(100),
        )],
    )
    .unwrap();
    let mut valid = sealed_receipt(
        key.clone(),
        ReceiptDelta::sealed(ResVec::new(Money::ZERO, 100), ResVec::ZERO, ResVec::ZERO),
        0,
    );
    valid.deliver_qty = 0;
    ledger.apply(std::slice::from_mut(&mut valid)).unwrap();

    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key.clone(),
            Money::ZERO,
            100,
            audit_with_remaining(100),
        )],
    )
    .unwrap();
    let mut invalid = sealed_receipt(
        key,
        ReceiptDelta::sealed(
            ResVec::new(Money::ZERO, 99),
            ResVec::new(Money::ZERO, 1),
            ResVec::ZERO,
        ),
        0,
    );
    invalid.deliver_qty = 0;
    assert!(ledger.apply(std::slice::from_mut(&mut invalid)).is_err());
}

#[test]
fn a_terminal_fill_cannot_leave_live_resources() {
    let mut ledger = created_ledger();
    let mut receipt = receipt(0);
    receipt.value_after = Money::from_cents(90);
    receipt.deliver_qty = 100;
    receipt.delta = ReceiptDelta::sealed(
        ResVec::new(Money::from_cents(90), 0),
        ResVec::ZERO,
        ResVec::new(Money::from_cents(10), 0),
    );

    let error = ledger
        .apply(std::slice::from_mut(&mut receipt))
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("terminal receipt leaves live resources"));
}
#[test]
fn auction_rollover_chain_must_continue_into_day_end() {
    let key = key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::p3_created(
            key.clone(),
            Money::from_cents(100),
            0,
            audit_with_remaining(100),
        )],
    )
    .unwrap();
    let before = ledger.clone();
    let rollover = receipt_from_source(ReceiptFixture {
        key: key.clone(),
        source: ReceiptSource::Auction(0),
        kind: ReceiptKind::Rollover,
        qty_before: 100,
        qty_after: 100,
        value_before: Money::ZERO,
        value_after: Money::ZERO,
        delta: ReceiptDelta::sealed(
            ResVec::ZERO,
            ResVec::ZERO,
            ResVec::new(Money::from_cents(100), 0),
        ),
    });
    let day_end = receipt_from_source(ReceiptFixture {
        key,
        source: ReceiptSource::DayEnd(0),
        kind: ReceiptKind::Release,
        qty_before: 99,
        qty_after: 0,
        value_before: Money::ZERO,
        value_after: Money::ZERO,
        delta: ReceiptDelta::sealed(
            ResVec::ZERO,
            ResVec::new(Money::from_cents(100), 0),
            ResVec::ZERO,
        ),
    });
    let mut receipts = [rollover, day_end];
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
}
