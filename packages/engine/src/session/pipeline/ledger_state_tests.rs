use super::ledger_tests::*;
use super::{
    transition::{FillTransition, SellFillInput},
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeOrigin, EnvelopeReceipt,
    FeeComponents, JournalRank, ReceiptKind, ReceiptLocalKey, ReceiptSource, ReceiptTransition,
    ResVec,
};
use crate::{GameConfig, Money};

#[test]
fn active_rebase_keeps_untouched_pages_shared_and_preserves_fee_audit() {
    let first = existing(key(), Money::from_cents(100));
    let mut far_key = key();
    far_key.account = crate::AccountId(512);
    far_key.order = crate::OrderId(512);
    let untouched = existing(far_key.clone(), Money::from_cents(200));
    let mut candidate = EnvelopeLedger::new(7, [first.clone(), untouched.clone()]).unwrap();
    let mut release = expiry_release(first.key().clone(), ResVec::new(Money::from_cents(100), 0));
    candidate.apply(std::slice::from_mut(&mut release)).unwrap();
    candidate
        .remove_terminal(std::slice::from_ref(first.key()))
        .unwrap();
    let before = candidate.clone();
    candidate.rebase_private_for_tick_commit().unwrap();
    assert!(
        std::ptr::eq(
            before.get(&far_key).unwrap(),
            candidate.get(&far_key).unwrap()
        ),
        "处理其他账户的终结行不应重建未触及 page"
    );
    assert_eq!(candidate.get(&far_key).unwrap(), &untouched);
    assert_eq!(candidate.next_receipt_index(), before.next_receipt_index());
    assert_eq!(candidate.terminal_count(), 0);
    candidate.validate_complete_evidence().unwrap();
}

#[test]
fn hydration_rejects_same_length_field_key_and_terminal_mismatches_atomically() {
    let first = existing(key(), Money::from_cents(100));
    let second =
        Envelope::tick_start_existing(second_key(), Money::ZERO, 100, audit_with_remaining(100));
    let authority = EnvelopeLedger::new(7, [first.clone(), second.clone()]).unwrap();
    let field = existing(key(), Money::from_cents(101));
    let changed_audit = Envelope::tick_start_existing(
        key(),
        Money::from_cents(100),
        0,
        EnvelopeAudit {
            limit: Money::from_cents(2),
            ..first.audit()
        },
    );
    for projected in [
        vec![field, second.clone()],
        vec![changed_audit, second.clone()],
        vec![first.clone(), extra()],
    ] {
        let mut candidate = authority.clone();
        assert!(candidate.hydrate_or_validate(projected).is_err());
        assert_eq!(candidate, authority);
    }
    let mut empty = EnvelopeLedger::new(7, []).unwrap();
    let before = empty.clone();
    assert!(empty
        .hydrate_or_validate([first.clone(), first.clone()])
        .is_err());
    assert_eq!(empty, before);
    for mut candidate in [empty, authority] {
        candidate
            .terminal_envelopes
            .insert(first.key().clone(), first.clone());
        let before = candidate.clone();
        assert!(
            matches!(candidate.hydrate_or_validate([first.clone(), second.clone()]),
            Err(super::StepFatal::InvariantViolation { description, .. })
                if description == "live projection overlaps terminal envelope evidence")
        );
        assert_eq!(candidate, before);
    }
}

#[test]
fn full_evidence_parallel_threshold_and_mixed_terminal_rows_preserve_first_error() {
    for count in [511_u64, 512, 513] {
        let values = (0..count).map(|index| {
            Envelope::tick_start_existing(
                EnvelopeKey {
                    account: crate::AccountId(index),
                    stock: crate::StockId(0),
                    order: crate::OrderId(index),
                    side: crate::Side::Buy,
                },
                Money::from_cents(1),
                0,
                audit(),
            )
        });
        let mut ledger = EnvelopeLedger::new(7, values).unwrap();
        // 用真实 release transition 建立 terminal 行，保留累积守恒证据。
        let terminal_key = ledger.envelopes.keys().next_back().unwrap().clone();
        let mut release =
            expiry_release(terminal_key.clone(), ResVec::new(Money::from_cents(1), 0));
        ledger.apply(std::slice::from_mut(&mut release)).unwrap();
        ledger
            .remove_terminal(std::slice::from_ref(&terminal_key))
            .unwrap();
        assert_eq!(ledger.terminal_count(), 1);
        ledger.validate_complete_evidence().unwrap();
        let early = ledger.iter().next().unwrap().0.clone();
        let later = ledger.iter().nth(400).unwrap().0.clone();
        ledger.audits.get_mut(&early).unwrap().limit = Money::from_cents(2);
        // 后续行制造不同的 overlap 错误，证据 map 的数量仍与行数对应。
        // 应先报告更早行的 audit mismatch，而不是按 worker 完成顺序选错。
        ledger.terminal_envelopes.insert(
            later.clone(),
            Envelope::tick_start_existing(later, Money::ZERO, 0, audit()),
        );
        let extra_key = EnvelopeKey {
            account: crate::AccountId(count + 1),
            stock: crate::StockId(0),
            order: crate::OrderId(count + 1),
            side: crate::Side::Buy,
        };
        ledger.audits.insert(extra_key.clone(), audit());
        ledger
            .conservation
            .insert(extra_key, super::ledger::ConservationState::EMPTY);
        let before = ledger.clone();
        for threads in [1, 4] {
            let error = crate::compute::ComputeContext::from_workers(threads)
                .install(|| ledger.validate_complete_evidence())
                .unwrap_err();
            assert!(
                matches!(error, super::StepFatal::InvariantViolation { description, .. }
                if description == "envelope audit row disagrees with envelope")
            );
            assert_eq!(ledger, before);
        }
    }
}

#[test]
fn nonempty_hydration_checks_projection_without_constructing_a_replacement_ledger() {
    let first = existing(key(), Money::from_cents(100));
    let second =
        Envelope::tick_start_existing(second_key(), Money::ZERO, 100, audit_with_remaining(100));
    let mut authority = EnvelopeLedger::new(7, [first.clone(), second.clone()]).unwrap();
    let before = authority.clone();
    super::ledger::LEDGER_CONSTRUCTION_COUNT.with(|count| count.set(0));
    authority
        .hydrate_or_validate([second.clone(), first.clone()])
        .unwrap();
    assert_eq!(authority, before);
    assert_eq!(
        super::ledger::LEDGER_CONSTRUCTION_COUNT.with(|count| count.get()),
        0,
        "只验证已有 ledger，不应重新构造全部证据行"
    );
    assert!(authority
        .hydrate_or_validate([first.clone(), second.clone(), first.clone()])
        .is_err());
    assert_eq!(authority, before);
    assert!(authority.hydrate_or_validate([first]).is_err());
    assert_eq!(authority, before);
}

#[test]
fn unchanged_tick_rebase_keeps_live_rows_shared_without_discarding_audit() {
    let envelope = existing(key(), Money::from_cents(100));
    let authority = EnvelopeLedger::new(7, [envelope.clone()]).unwrap();
    let mut candidate = authority.clone();
    candidate.rebase_private_for_tick_commit().unwrap();
    assert!(
        std::ptr::eq(
            authority.get(envelope.key()).unwrap(),
            candidate.get(envelope.key()).unwrap()
        ),
        "未变化的跨 tick 行不能在 rebase 中重复复制"
    );
    assert_eq!(candidate.get(envelope.key()).unwrap(), &envelope);
    candidate.validate_complete_evidence().unwrap();
}

#[test]
fn ledger_clone_shares_live_rows_until_mutation_and_keeps_authority_isolated() {
    let envelope = existing(key(), Money::from_cents(100));
    let authority = EnvelopeLedger::new(7, [envelope.clone()]).unwrap();
    let mut candidate = authority.clone();
    assert!(
        std::ptr::eq(
            authority.get(envelope.key()).unwrap(),
            candidate.get(envelope.key()).unwrap()
        ),
        "未修改的 live ledger 必须共享不可变行"
    );
    candidate.envelopes.remove(envelope.key());
    assert_eq!(authority.get(envelope.key()).unwrap(), &envelope);
    assert!(candidate.get(envelope.key()).is_err());
}

#[test]
fn evidence_validation_reuses_immutable_ledger_and_rechecks_mutated_maps() {
    let envelope = existing(key(), Money::from_cents(100));
    let authority = EnvelopeLedger::new(7, [envelope.clone()]).unwrap();
    super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.set(0));
    authority.validate_complete_evidence().unwrap();
    let candidate = authority.clone();
    candidate.validate_complete_evidence().unwrap();
    assert_eq!(
        super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.get()),
        1,
        "相同 ledger 不应重复全量审计"
    );
    let corruptions: [fn(&mut EnvelopeLedger); 4] = [
        |ledger| {
            ledger.envelopes.remove(&key());
        },
        |ledger| {
            ledger.audits.remove(&key());
        },
        |ledger| {
            ledger.conservation.remove(&key());
        },
        |ledger| {
            let value = ledger.envelopes.get(&key()).unwrap().clone();
            ledger.terminal_envelopes.insert(key(), value);
        },
    ];
    for corrupt in corruptions {
        let mut changed = authority.clone();
        corrupt(&mut changed);
        assert!(
            changed.validate_complete_evidence().is_err(),
            "mutation 后不得复用旧成功审计"
        );
        authority.validate_complete_evidence().unwrap();
    }
    assert_eq!(authority.get(envelope.key()).unwrap(), &envelope);
}

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
    let origin_mismatch =
        Envelope::created_at_validation(key(), Money::from_cents(100), 0, audit());
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
fn conservation_quote_expiry_double_release_is_fatal_without_mutation() {
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
        ReceiptSource::QuoteExpiry(1),
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
        [Envelope::created_at_validation(
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
    // 无新成交的下一 tick 使用 fastpath，仍需保留名义费用、实收及历史欠费。
    ledger.rebase_private_for_tick_commit().unwrap();
    assert!(std::ptr::eq(
        authoritative.get(&key).unwrap(),
        ledger.get(&key).unwrap()
    ));
    assert_eq!(ledger.get(&key).unwrap().audit(), audit_after_first);
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
    ledger
        .remove_terminal(std::slice::from_ref(&terminal_key))
        .unwrap();
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
    let zero_live =
        Envelope::created_at_validation(second_key(), Money::ZERO, 0, audit_with_remaining(1));
    let invalid_resources = Envelope::created_at_validation(
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
        Envelope::created_at_validation(second_key(), Money::ZERO, 0, audit()),
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
        [Envelope::created_at_validation(
            overlap_key.clone(),
            Money::ZERO,
            2,
            overlap_audit,
        )],
    )
    .unwrap();
    overlapping.terminal_envelopes.insert(
        overlap_key.clone(),
        Envelope::created_at_validation(overlap_key, Money::ZERO, 0, overlap_audit),
    );
    overlapping.audits.insert(third_key(), audit());
    overlapping
        .conservation
        .insert(third_key(), super::ledger::ConservationState::EMPTY);

    let zero_live_key = key();
    let mut zero_live = EnvelopeLedger::new(91, []).unwrap();
    let zero_live_envelope = Envelope::created_at_validation(
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
        stock: crate::StockId(2),
        order: crate::OrderId(3),
        side: crate::Side::Buy,
    }
}

fn created_seller(key: EnvelopeKey, shares: u32) -> Envelope {
    Envelope::created_at_validation(
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
    Envelope::created_at_validation(
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

#[test]
fn borrowed_projection_gate_preserves_owned_gate_errors_and_map_key_checks() {
    let first = existing(key(), Money::from_cents(100));
    let second =
        Envelope::tick_start_existing(second_key(), Money::ZERO, 100, audit_with_remaining(100));
    let authority = EnvelopeLedger::new(7, [first.clone(), second.clone()]).unwrap();
    let field = existing(key(), Money::from_cents(101));
    let audit = Envelope::tick_start_existing(
        key(),
        Money::from_cents(100),
        0,
        EnvelopeAudit {
            limit: Money::from_cents(2),
            ..first.audit()
        },
    );
    let mut terminal = authority.clone();
    terminal
        .terminal_envelopes
        .insert(first.key().clone(), first.clone());
    let mut wrong_map_key = authority.clone();
    *wrong_map_key.envelopes.get_mut(first.key()).unwrap() = second.clone();
    for source in [authority, terminal, wrong_map_key] {
        for mut projections in [
            vec![first.clone(), second.clone()],
            vec![field.clone(), second.clone()],
            vec![audit.clone(), second.clone()],
            vec![first.clone()],
            vec![first.clone(), extra()],
            vec![first.clone(), first.clone()],
            vec![],
        ] {
            projections.sort_by(|left, right| left.key().cmp(right.key()));
            let borrowed = projections
                .iter()
                .map(|envelope| super::BorrowedTickStartEnvelope {
                    account: envelope.key().account,
                    stock: &envelope.key().stock,
                    order: envelope.key().order,
                    side: envelope.key().side,
                    live: envelope.live(),
                    audit: envelope.audit(),
                })
                .collect::<Vec<_>>();
            let mut owned_candidate = source.clone();
            let owned_result = owned_candidate.hydrate_or_validate(projections.clone());
            let borrowed_result = source.validate_borrowed_tick_start_projection(&borrowed);
            assert_eq!(borrowed_result, owned_result);
        }
    }
}

#[test]
fn active_tick_rebase_carries_complete_evidence_proof_into_the_next_read_only_gate() {
    let first = existing(key(), Money::from_cents(100));
    let mut untouched_key = key();
    untouched_key.stock = crate::StockId(2);
    untouched_key.order = crate::OrderId(3);
    let untouched = existing(untouched_key, Money::from_cents(200));
    let mut ledger = EnvelopeLedger::new(41, [first.clone(), untouched.clone()]).unwrap();
    let mut receipt = expiry_release(first.key().clone(), ResVec::new(Money::from_cents(100), 0));
    ledger.apply(std::slice::from_mut(&mut receipt)).unwrap();
    ledger
        .remove_terminal(std::slice::from_ref(first.key()))
        .unwrap();
    ledger.rebase_private_for_tick_commit().unwrap();
    let before = ledger.clone();
    super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.set(0));
    ledger.validate_complete_evidence().unwrap();
    assert_eq!(
        super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.get()),
        0,
        "rebase 已校验来源与全部变化行，应复用这份完整证据而非立即再次扫描"
    );
    assert_eq!(ledger, before);
    assert_eq!(ledger.get(untouched.key()).unwrap(), &untouched);
    assert_eq!(ledger.terminal_count(), 0);
    assert_eq!(ledger.next_receipt_index(), 42);
}

#[test]
fn rebase_proof_preserves_seller_fee_debt_and_invalidates_on_each_map_mutation() {
    let config = GameConfig::proposed_defaults();
    let seller_key = second_key();
    let mut ledger = EnvelopeLedger::new(
        41,
        [Envelope::created_at_validation(
            seller_key.clone(),
            Money::ZERO,
            3,
            EnvelopeAudit {
                limit: Money::from_cents(1000),
                remaining_qty: 3,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )],
    )
    .unwrap();
    let mut receipt = seller_fill_receipt(
        &config,
        seller_key.clone(),
        ledger.get(&seller_key).unwrap().audit(),
        100,
    );
    ledger.apply(std::slice::from_mut(&mut receipt)).unwrap();
    let audit = ledger.get(&seller_key).unwrap().audit();
    assert!(audit.nominal.total().unwrap() > audit.charged.total().unwrap());
    ledger.rebase_private_for_tick_commit().unwrap();
    let expected = EnvelopeLedger::new(
        42,
        [Envelope::tick_start_existing(
            seller_key.clone(),
            Money::ZERO,
            2,
            audit,
        )],
    )
    .unwrap();
    assert_eq!(ledger, expected);
    assert_eq!(
        serde_json::to_vec(&ledger.hash_projection()).unwrap(),
        serde_json::to_vec(&expected.hash_projection()).unwrap()
    );
    super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.set(0));
    ledger.validate_complete_evidence().unwrap();
    assert_eq!(
        super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.get()),
        0
    );
    // 独立新建的等值 ledger 没有该构造证明，必须执行原完整门禁。
    expected.validate_complete_evidence().unwrap();
    let corruptions: [fn(&mut EnvelopeLedger, &EnvelopeKey); 4] = [
        |ledger, key| {
            ledger.envelopes.remove(key);
        },
        |ledger, key| {
            ledger.audits.remove(key);
        },
        |ledger, key| {
            ledger.conservation.remove(key);
        },
        |ledger, key| {
            let row = ledger.envelopes.get(key).unwrap().clone();
            ledger.terminal_envelopes.insert(key.clone(), row);
        },
    ];
    for corrupt in corruptions {
        let mut changed = ledger.clone();
        corrupt(&mut changed, &seller_key);
        super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.set(0));
        assert!(changed.validate_complete_evidence().is_err());
        assert_eq!(
            super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.get()),
            1
        );
        ledger.validate_complete_evidence().unwrap();
        assert_eq!(ledger, expected);
    }
}

#[test]
fn rebase_proof_handles_exact_cash_share_totals_and_all_terminal_removal() {
    for shares in [false, true] {
        let identity = if shares { second_key() } else { key() };
        let mut ledger = EnvelopeLedger::new(
            77,
            [Envelope::created_at_validation(
                identity.clone(),
                if shares {
                    Money::ZERO
                } else {
                    Money::from_cents(i64::MAX)
                },
                if shares { u32::MAX } else { 0 },
                audit(),
            )],
        )
        .unwrap();
        ledger.rebase_private_for_tick_commit().unwrap();
        super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.set(0));
        ledger.validate_complete_evidence().unwrap();
        assert_eq!(
            super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.get()),
            0
        );
        let expected = EnvelopeLedger::new(
            77,
            [Envelope::tick_start_existing(
                identity,
                if shares {
                    Money::ZERO
                } else {
                    Money::from_cents(i64::MAX)
                },
                if shares { u32::MAX } else { 0 },
                audit(),
            )],
        )
        .unwrap();
        expected.validate_complete_evidence().unwrap();
        assert_eq!(ledger, expected);
    }
    let first = existing(key(), Money::from_cents(100));
    let mut ledger = EnvelopeLedger::new(41, [first.clone()]).unwrap();
    let mut release = expiry_release(first.key().clone(), first.live());
    ledger.apply(std::slice::from_mut(&mut release)).unwrap();
    ledger
        .remove_terminal(std::slice::from_ref(first.key()))
        .unwrap();
    ledger.rebase_private_for_tick_commit().unwrap();
    super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.set(0));
    ledger.validate_complete_evidence().unwrap();
    assert_eq!(
        super::ledger::EVIDENCE_VALIDATION_COUNT.with(|count| count.get()),
        0
    );
    assert_eq!(ledger, EnvelopeLedger::new(42, []).unwrap());
}

#[test]
fn dense_complete_evidence_uses_one_parallel_row_mapping_without_changing_the_ledger() {
    let rows = (0..513).map(|index| {
        let mut envelope_key = key();
        envelope_key.account = crate::AccountId(index);
        envelope_key.order = crate::OrderId(index);
        existing(envelope_key, Money::from_cents(1))
    });
    let ledger = EnvelopeLedger::new(0, rows).unwrap();
    let before = ledger.clone();
    super::ledger_conservation::ROW_MAPPING_PASSES.with(|count| count.set(0));
    ledger.validate_complete_evidence().unwrap();
    super::ledger_conservation::ROW_MAPPING_PASSES.with(|count| {
        assert_eq!(
            count.get(),
            1,
            "完整门禁应在一轮并行映射中保留守恒和证据校验，避免重复枚举与等待"
        )
    });
    assert_eq!(ledger, before);
}

#[test]
fn complete_evidence_defers_early_audit_error_until_conservation_resources_and_shape_pass() {
    for count in [511, 512, 513] {
        let rows = (0..count).map(|index| {
            let mut envelope_key = key();
            envelope_key.account = crate::AccountId(index);
            envelope_key.order = crate::OrderId(index);
            existing(envelope_key, Money::from_cents(1))
        });
        let source = EnvelopeLedger::new(0, rows).unwrap();
        let first = source.envelopes.keys().next().unwrap().clone();
        let last = source.envelopes.keys().next_back().unwrap().clone();
        for fault in 0..4 {
            let mut ledger = source.clone();
            ledger.audits.get_mut(&first).unwrap().limit = Money::from_cents(2);
            let description = match fault {
                0 => {
                    ledger.conservation.remove(&last);
                    "missing conservation row"
                }
                1 => {
                    ledger
                        .envelopes
                        .insert(last.clone(), existing(last.clone(), Money::ZERO));
                    "live envelope row has no live resources"
                }
                2 => {
                    let envelope = ledger.envelopes.remove(&last).unwrap();
                    ledger.terminal_envelopes.insert(last.clone(), envelope);
                    "terminal envelope row still has live resources"
                }
                3 => {
                    ledger.audits.remove(&last);
                    "ledger evidence rows do not match envelope rows"
                }
                _ => unreachable!(),
            };
            let before = ledger.clone();
            for threads in [1, 4] {
                let error = crate::compute::ComputeContext::from_workers(threads)
                    .install(|| ledger.validate_complete_evidence())
                    .unwrap_err();
                assert_eq!(
                    error,
                    super::ledger_validation::invariant(description),
                    "{count}行/{threads}worker/fault{fault}应保留全门禁错误优先级"
                );
                assert_eq!(ledger, before);
            }
        }
    }
}
