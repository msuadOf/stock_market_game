use super::ledger_tests::*;
use super::{
    transition::{FillTransition, SellFillInput},
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeOrigin, EnvelopeReceipt,
    FeeComponents, JournalRank, ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition,
    ResVec,
};
use crate::{GameConfig, Money};

#[test]
fn hydration_installs_empty_or_validates_exact_without_cursor_change() {
    let envelope = existing(key(), Money::from_cents(100));
    let mut ledger = EnvelopeLedger::new(7, []).unwrap();
    ledger.hydrate_or_validate([envelope.clone()]).unwrap();
    assert_eq!(ledger.next_receipt_index(), 7);
    assert_eq!(ledger.get(envelope.key()).unwrap(), &envelope);
    let before = ledger.clone();
    ledger.hydrate_or_validate([envelope]).unwrap();
    assert_eq!(ledger, before);
}
#[test]
fn hydration_rejects_every_mismatch_without_mutation() {
    let first = existing(key(), Money::from_cents(100));
    let second = existing(second_key(), Money::ZERO);
    let mut ledger = EnvelopeLedger::new(7, [first.clone(), second.clone()]).unwrap();
    let before = ledger.clone();
    let resource = existing(key(), Money::from_cents(101));
    let mut changed = audit();
    changed.remaining_qty = 1;
    let audit_mismatch = Envelope::tick_start_existing(key(), Money::from_cents(100), 0, changed);
    let origin_mismatch = Envelope::p3_created(key(), Money::from_cents(100), 0, audit());
    for projected in [
        vec![first.clone()],
        vec![first.clone(), second.clone(), extra()],
        vec![resource],
        vec![audit_mismatch],
        vec![origin_mismatch],
    ] {
        assert!(ledger.hydrate_or_validate(projected).is_err());
        assert_eq!(ledger, before);
    }
}
#[test]
fn preseal_expiry_release_consumes_buy_live_cash_without_spending() {
    let key = key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::tick_start_existing(
            key.clone(),
            Money::from_cents(100),
            0,
            audit(),
        )],
    )
    .unwrap();
    let mut receipts = [expiry_release(key, ResVec::new(Money::from_cents(100), 0))];
    ledger.apply(&mut receipts).unwrap();
    assert_eq!(receipts[0].index, 7);
    assert_eq!(
        ledger.get(&receipts[0].envelope).unwrap().live(),
        ResVec::ZERO
    );
}

#[test]
fn conservation_p0_double_release_is_fatal_without_mutation() {
    let key = key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::tick_start_existing(
            key.clone(),
            Money::from_cents(100),
            0,
            audit(),
        )],
    )
    .unwrap();
    let before = ledger.clone();
    let mut receipts = [
        expiry_release(key.clone(), ResVec::new(Money::from_cents(100), 0)),
        expiry_release(key, ResVec::new(Money::from_cents(100), 0)),
    ];
    receipts[1].local_key = ReceiptLocalKey::new(
        JournalRank::PreSeal,
        ReceiptSource::P0Expiry(1),
        ReceiptTransition {
            envelope: receipts[1].envelope.clone(),
            ordinal: 0,
        },
    )
    .unwrap();

    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
}
#[test]
fn terminal_row_is_retained_until_an_explicit_tick_reset() {
    let key = key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [Envelope::tick_start_existing(
            key.clone(),
            Money::from_cents(100),
            0,
            audit(),
        )],
    )
    .unwrap();
    let mut receipts = [expiry_release(
        key.clone(),
        ResVec::new(Money::from_cents(100), 0),
    )];
    ledger.apply(&mut receipts).unwrap();
    ledger.remove_terminal(&[key]).unwrap();
    assert_eq!(ledger.terminal_count(), 1);
    ledger.validate_conservation().unwrap();
    ledger.reset_tick_state();
    assert_eq!(ledger.terminal_count(), 0);
}

#[test]
fn next_tick_rebase_preserves_cumulative_fee_audit_and_receipt_cursor() {
    let config = GameConfig::proposed_defaults();
    let key = second_key();
    let mut ledger = EnvelopeLedger::new(
        41,
        [Envelope::p3_created(
            key.clone(),
            Money::ZERO,
            3,
            EnvelopeAudit {
                limit: Money::from_cents(1_000),
                remaining_qty: 3,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )],
    )
    .unwrap();
    let mut first =
        seller_fill_receipt(&config, key.clone(), ledger.get(&key).unwrap().audit(), 100);
    ledger.apply(std::slice::from_mut(&mut first)).unwrap();
    assert_eq!(first.index, 41);
    let audit_after_first = ledger.get(&key).unwrap().audit();
    assert!(
        audit_after_first.nominal.total().unwrap() > audit_after_first.charged.total().unwrap()
    );

    ledger.rebase_live_for_next_tick().unwrap();

    let rebased = ledger.get(&key).unwrap();
    assert_eq!(rebased.origin(), EnvelopeOrigin::TickStart);
    assert_eq!(rebased.basis(), ResVec::new(Money::ZERO, 2));
    assert_eq!(rebased.live(), ResVec::new(Money::ZERO, 2));
    assert_eq!(rebased.spent(), ResVec::ZERO);
    assert_eq!(rebased.released(), ResVec::ZERO);
    assert_eq!(rebased.audit(), audit_after_first);
    assert_eq!(ledger.next_receipt_index(), 42);

    let authoritative = ledger.clone();
    let projected_without_fee_history = Envelope::tick_start_existing(
        key.clone(),
        Money::ZERO,
        2,
        EnvelopeAudit {
            nominal: FeeComponents::ZERO,
            charged: FeeComponents::ZERO,
            ..audit_after_first
        },
    );
    assert!(ledger
        .hydrate_or_validate([projected_without_fee_history])
        .is_err());
    assert_eq!(ledger, authoritative);

    // The same tick-local key may be reused in the next tick. The second leg must
    // start from the preserved cumulative nominal/charged history so it can collect
    // the first leg's unpaid nominal fee instead of silently resetting that debt.
    let mut second = seller_fill_receipt(&config, key.clone(), audit_after_first, 1_000);
    assert!(second.charged.total().unwrap() > second.nominal.total().unwrap());
    ledger.apply(std::slice::from_mut(&mut second)).unwrap();
    assert_eq!(second.index, 42);
    let final_audit = ledger.get(&key).unwrap().audit();
    assert_eq!(final_audit.filled_qty, 2);
    assert_eq!(final_audit.remaining_qty, 1);
    assert_eq!(final_audit.filled_value, Money::from_cents(1_100));
    assert_eq!(final_audit.charged, second.charged_after);
}

#[test]
fn next_tick_rebase_clears_only_tick_local_ledger_state() {
    let live_key = second_key();
    let terminal_key = key();
    let mut ledger = EnvelopeLedger::new(
        7,
        [
            Envelope::tick_start_existing(
                live_key.clone(),
                Money::ZERO,
                2,
                EnvelopeAudit {
                    limit: Money::from_cents(1_000),
                    remaining_qty: 2,
                    filled_qty: 1,
                    filled_value: Money::from_cents(100),
                    nominal: FeeComponents {
                        commission: Money::from_cents(500),
                        stamp_tax: Money::ZERO,
                        transfer_fee: Money::ZERO,
                    },
                    charged: FeeComponents {
                        commission: Money::from_cents(100),
                        stamp_tax: Money::ZERO,
                        transfer_fee: Money::ZERO,
                    },
                },
            ),
            Envelope::tick_start_existing(terminal_key.clone(), Money::from_cents(100), 0, audit()),
        ],
    )
    .unwrap();
    let mut terminal_receipt =
        expiry_release(terminal_key.clone(), ResVec::new(Money::from_cents(100), 0));
    ledger
        .apply(std::slice::from_mut(&mut terminal_receipt))
        .unwrap();
    ledger.remove_terminal(&[terminal_key.clone()]).unwrap();
    assert_eq!(ledger.terminal_count(), 1);
    assert!(!ledger.seen_local_keys.is_empty());

    let live_audit = ledger.get(&live_key).unwrap().audit();
    let mut private_candidate = ledger.clone();
    ledger.rebase_live_for_next_tick().unwrap();
    private_candidate.rebase_private_for_tick_commit().unwrap();
    assert_eq!(private_candidate, ledger);

    assert_eq!(ledger.terminal_count(), 0);
    assert!(ledger.get(&terminal_key).is_err());
    assert!(ledger.seen_local_keys.is_empty());
    assert_eq!(ledger.audits.len(), 1);
    assert_eq!(ledger.audits.get(&live_key), Some(&live_audit));
    assert_eq!(ledger.conservation.len(), 1);
    assert_eq!(
        ledger.conservation.get(&live_key),
        Some(&super::ledger::ConservationState::EMPTY)
    );
    assert_eq!(ledger.next_receipt_index(), 8);
    ledger.validate_conservation().unwrap();
}

#[test]
fn next_tick_rebase_failure_is_atomic() {
    let mut ledger = created_ledger();
    ledger
        .audits
        .get_mut(&key())
        .expect("fixture envelope has an audit row")
        .remaining_qty = 99;
    let before = ledger.clone();

    assert!(ledger.rebase_live_for_next_tick().is_err());
    assert_eq!(ledger, before);
}

#[test]
fn insert_created_adds_complete_rows_without_changing_cursor_or_seen_keys() {
    let existing_key = key();
    let mut ledger = EnvelopeLedger::new(
        31,
        [Envelope::tick_start_existing(
            existing_key.clone(),
            Money::from_cents(100),
            0,
            audit_with_remaining(1),
        )],
    )
    .unwrap();
    let seen = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(9),
        ReceiptTransition {
            envelope: existing_key,
            ordinal: 0,
        },
    )
    .unwrap();
    ledger.seen_local_keys.insert(seen.clone());
    let seller = created_seller(second_key(), 2);
    let buyer = created_buyer(third_key(), Money::from_cents(100), 1);

    ledger
        .insert_created(vec![seller.clone(), buyer.clone()])
        .unwrap();

    assert_eq!(ledger.get(seller.key()).unwrap(), &seller);
    assert_eq!(ledger.get(buyer.key()).unwrap(), &buyer);
    assert_eq!(ledger.audits.get(seller.key()), Some(&seller.audit()));
    assert_eq!(ledger.audits.get(buyer.key()), Some(&buyer.audit()));
    assert_eq!(
        ledger.conservation.get(seller.key()),
        Some(&super::ledger::ConservationState::EMPTY)
    );
    assert_eq!(
        ledger.conservation.get(buyer.key()),
        Some(&super::ledger::ConservationState::EMPTY)
    );
    assert_eq!(ledger.next_receipt_index(), 31);
    assert_eq!(ledger.seen_local_keys, [seen].into());
    ledger.validate_conservation().unwrap();
}

#[test]
fn insert_created_rejects_invalid_envelopes_atomically() {
    let invalid_origin =
        Envelope::tick_start_existing(second_key(), Money::ZERO, 1, audit_with_remaining(1));
    let zero_live = Envelope::p3_created(second_key(), Money::ZERO, 0, audit_with_remaining(1));
    let invalid_resources = Envelope::p3_created(
        second_key(),
        Money::from_cents(-1),
        0,
        audit_with_remaining(1),
    );
    for invalid in [invalid_origin, zero_live, invalid_resources] {
        let mut ledger = created_ledger();
        let before = ledger.clone();
        assert!(ledger.insert_created([invalid]).is_err());
        assert_eq!(ledger, before);
    }
}

#[test]
fn insert_created_rejects_batch_duplicates_atomically() {
    let duplicate = created_seller(second_key(), 2);
    let mut ledger = created_ledger();
    let before = ledger.clone();

    assert!(ledger
        .insert_created([duplicate.clone(), duplicate])
        .is_err());
    assert_eq!(ledger, before);
}

#[test]
fn insert_created_rejects_every_existing_key_domain_atomically() {
    let conflict = created_seller(second_key(), 2);

    let live = EnvelopeLedger::new(7, [conflict.clone()]).unwrap();

    let mut terminal = EnvelopeLedger::new(7, []).unwrap();
    terminal.terminal_envelopes.insert(
        second_key(),
        Envelope::p3_created(second_key(), Money::ZERO, 0, audit()),
    );
    terminal.audits.insert(second_key(), audit());
    terminal
        .conservation
        .insert(second_key(), super::ledger::ConservationState::EMPTY);

    let mut audit_only = EnvelopeLedger::new(7, []).unwrap();
    audit_only.audits.insert(second_key(), audit());

    let mut conservation_only = EnvelopeLedger::new(7, []).unwrap();
    conservation_only
        .conservation
        .insert(second_key(), super::ledger::ConservationState::EMPTY);

    for mut ledger in [live, terminal, audit_only, conservation_only] {
        let before = ledger.clone();
        assert!(ledger.insert_created([conflict.clone()]).is_err());
        assert_eq!(ledger, before);
    }
}

#[test]
fn private_stock_round_insertion_checks_the_whole_batch_before_writing() {
    let mut ledger = created_ledger();
    let before = ledger.clone();
    let seller = created_seller(second_key(), 2);
    assert!(ledger
        .insert_created_for_stock_round([seller.clone(), seller.clone()])
        .is_err());
    assert_eq!(ledger, before);

    ledger
        .insert_created_for_stock_round([seller.clone()])
        .unwrap();
    assert_eq!(ledger.get(seller.key()).unwrap(), &seller);
    ledger.validate_complete_evidence().unwrap();
}

#[test]
fn complete_evidence_validation_rejects_every_structural_gap_without_mutation() {
    let mut orphan_audit = EnvelopeLedger::new(91, []).unwrap();
    orphan_audit.audits.insert(key(), audit());

    let mut orphan_conservation = EnvelopeLedger::new(91, []).unwrap();
    orphan_conservation
        .conservation
        .insert(key(), super::ledger::ConservationState::EMPTY);

    let mut missing_audit = EnvelopeLedger::new(91, [created_seller(key(), 2)]).unwrap();
    missing_audit.audits.remove(&key());

    let mut missing_conservation = EnvelopeLedger::new(91, [created_seller(key(), 2)]).unwrap();
    missing_conservation.conservation.remove(&key());

    let overlap_key = key();
    let overlap_audit = audit_with_remaining(2);
    let mut overlapping = EnvelopeLedger::new(
        91,
        [Envelope::p3_created(
            overlap_key.clone(),
            Money::ZERO,
            2,
            overlap_audit,
        )],
    )
    .unwrap();
    overlapping.terminal_envelopes.insert(
        overlap_key.clone(),
        Envelope::p3_created(overlap_key, Money::ZERO, 0, overlap_audit),
    );
    overlapping.audits.insert(third_key(), audit());
    overlapping
        .conservation
        .insert(third_key(), super::ledger::ConservationState::EMPTY);

    let zero_live_key = key();
    let mut zero_live = EnvelopeLedger::new(91, []).unwrap();
    let zero_live_envelope = Envelope::p3_created(
        zero_live_key.clone(),
        Money::ZERO,
        0,
        audit_with_remaining(1),
    );
    zero_live
        .envelopes
        .insert(zero_live_key.clone(), zero_live_envelope.clone());
    zero_live
        .audits
        .insert(zero_live_key.clone(), zero_live_envelope.audit());
    zero_live
        .conservation
        .insert(zero_live_key, super::ledger::ConservationState::EMPTY);

    let terminal_key = key();
    let mut nonzero_terminal = EnvelopeLedger::new(91, []).unwrap();
    let terminal_envelope = created_seller(terminal_key.clone(), 2);
    nonzero_terminal
        .terminal_envelopes
        .insert(terminal_key.clone(), terminal_envelope.clone());
    nonzero_terminal
        .audits
        .insert(terminal_key.clone(), terminal_envelope.audit());
    nonzero_terminal
        .conservation
        .insert(terminal_key, super::ledger::ConservationState::EMPTY);

    for ledger in [
        orphan_audit,
        orphan_conservation,
        missing_audit,
        missing_conservation,
        overlapping,
        zero_live,
        nonzero_terminal,
    ] {
        let before = ledger.clone();
        assert!(ledger.validate_complete_evidence().is_err());
        assert_eq!(ledger, before);
    }
}

#[test]
fn complete_evidence_validation_accepts_cross_tick_cumulative_fees_without_mutation() {
    let cumulative_audit = EnvelopeAudit {
        limit: Money::from_cents(1_000),
        remaining_qty: 2,
        filled_qty: 1,
        filled_value: Money::from_cents(100),
        nominal: FeeComponents {
            commission: Money::from_cents(500),
            stamp_tax: Money::ZERO,
            transfer_fee: Money::ZERO,
        },
        charged: FeeComponents {
            commission: Money::from_cents(100),
            stamp_tax: Money::ZERO,
            transfer_fee: Money::ZERO,
        },
    };
    let envelope_key = second_key();
    let mut ledger = EnvelopeLedger::new(
        92,
        [Envelope::tick_start_existing(
            envelope_key.clone(),
            Money::ZERO,
            2,
            cumulative_audit,
        )],
    )
    .unwrap();
    let seen = ReceiptLocalKey::new(
        JournalRank::SealedBatch,
        ReceiptSource::SealedIntent(17),
        ReceiptTransition {
            envelope: envelope_key,
            ordinal: 0,
        },
    )
    .unwrap();
    ledger.seen_local_keys.insert(seen);
    let before = ledger.clone();

    ledger.validate_complete_evidence().unwrap();

    assert_eq!(ledger, before);
    assert_eq!(ledger.next_receipt_index(), 92);
}

fn seller_fill_receipt(
    config: &GameConfig,
    key: EnvelopeKey,
    before: EnvelopeAudit,
    gross_cents: i64,
) -> EnvelopeReceipt {
    let remaining_qty = before.remaining_qty.checked_sub(1).unwrap();
    let gross = Money::from_cents(gross_cents);
    let transition = FillTransition::sell(SellFillInput {
        config,
        fill_qty: 1,
        remaining_qty_after: remaining_qty,
        filled_value_before: before.filled_value,
        gross_delta: gross,
        nominal_before: before.nominal,
        charged_before: before.charged,
    })
    .unwrap();
    EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(0),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )
        .unwrap(),
        envelope: key,
        kind: ReceiptKind::Fill,
        qty_before: before.remaining_qty,
        qty_after: remaining_qty,
        value_before: before.filled_value,
        value_after: before.filled_value.add(gross).unwrap(),
        delta: transition.delta,
        nominal: transition.nominal,
        charged: transition.charged,
        charged_before: before.charged,
        charged_after: transition.charged_after,
        deliver_qty: transition.deliver_qty,
        deliver_cash: transition.deliver_cash,
    }
}

fn third_key() -> EnvelopeKey {
    EnvelopeKey {
        account: crate::AccountId(2),
        stock: crate::StockCode("600003".to_owned()),
        order: crate::OrderId(3),
        side: crate::Side::Buy,
    }
}

fn created_seller(key: EnvelopeKey, shares: u32) -> Envelope {
    Envelope::p3_created(
        key,
        Money::ZERO,
        shares,
        EnvelopeAudit {
            limit: Money::from_cents(1_000),
            remaining_qty: shares,
            ..audit()
        },
    )
}

fn created_buyer(key: EnvelopeKey, cash: Money, remaining_qty: u32) -> Envelope {
    Envelope::p3_created(
        key,
        cash,
        0,
        EnvelopeAudit {
            limit: Money::from_cents(1_000),
            remaining_qty,
            ..audit()
        },
    )
}
#[test]
fn receipt_kind_and_source_boundary_is_checked_transactionally() {
    let mut ledger = created_ledger();
    let before = ledger.clone();
    let mut receipts = [receipt(0)];
    receipts[0].kind = ReceiptKind::Rollover;
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
}
#[test]
fn charged_component_discontinuity_is_rejected_transactionally() {
    let mut ledger = created_ledger();
    let before = ledger.clone();
    let mut receipts = [receipt(0)];
    receipts[0].charged_after = FeeComponents {
        commission: Money::from_cents(1),
        ..FeeComponents::ZERO
    };
    assert!(ledger.apply(&mut receipts).is_err());
    assert_eq!(ledger, before);
}
