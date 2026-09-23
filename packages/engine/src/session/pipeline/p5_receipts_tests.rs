use super::ledger_tests::*;
use super::p5_receipts::{apply_receipt_transaction, apply_session_receipt_transaction};
use super::{
    Envelope, EnvelopeLedger, JournalRank, ReceiptDelta, ReceiptKind, ReceiptLocalKey,
    ReceiptSource, ReceiptTransition, ResVec, StepFatal,
};
use crate::{GameSession, Money};

fn game_with_ledger(ledger: EnvelopeLedger) -> GameSession {
    let cursor = ledger.next_receipt_index();
    let mut game =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    game.envelope_ledger = ledger;
    game.next_receipt_base = cursor;
    game
}

#[test]
fn session_transaction_advances_ledger_and_authoritative_cursor_together() {
    let envelope_key = key();
    let mut game = game_with_ledger(created_ledger());

    let receipts = apply_session_receipt_transaction(
        &mut game,
        Vec::new(),
        vec![vec![receipt(0)]],
        vec![envelope_key],
    )
    .unwrap();

    assert_eq!(receipts[0].index, 7);
    assert_eq!(game.envelope_ledger.next_receipt_index(), 8);
    assert_eq!(game.next_receipt_base, 8);
}

#[test]
fn session_transaction_rejects_split_cursor_without_mutation() {
    let mut game = game_with_ledger(created_ledger());
    game.next_receipt_base = 6;
    let ledger_before = game.envelope_ledger.clone();

    let error = apply_session_receipt_transaction(
        &mut game,
        Vec::new(),
        vec![vec![receipt(0)]],
        vec![key()],
    )
    .unwrap_err();

    assert!(matches!(
        error,
        StepFatal::InvariantViolation {
            ref description,
            ref location,
        } if description.contains("session receipt cursor 6")
            && description.contains("envelope ledger cursor 7")
            && location == "pipeline::p5_receipts::apply_session_receipt_transaction"
    ));

    assert_eq!(game.envelope_ledger, ledger_before);
    assert_eq!(game.next_receipt_base, 6);
}

#[test]
fn failed_session_transaction_rolls_back_ledger_and_authoritative_cursor() {
    let mut game = game_with_ledger(created_ledger());
    let ledger_before = game.envelope_ledger.clone();

    assert!(apply_session_receipt_transaction(
        &mut game,
        Vec::new(),
        vec![vec![receipt(0)]],
        Vec::new(),
    )
    .is_err());

    assert_eq!(game.envelope_ledger, ledger_before);
    assert_eq!(game.next_receipt_base, 7);
}

#[test]
fn worker_indices_and_completion_order_are_rebuilt_canonically() {
    let first_key = key();
    let second_key = extra().key().clone();
    let mut ledger = EnvelopeLedger::new(
        41,
        [
            Envelope::p3_created(
                first_key.clone(),
                Money::from_cents(100),
                0,
                audit_with_remaining(100),
            ),
            Envelope::p3_created(
                second_key.clone(),
                Money::from_cents(100),
                0,
                audit_with_remaining(100),
            ),
        ],
    )
    .unwrap();
    let mut first = sealed_receipt(
        first_key.clone(),
        ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(100), 0),
            ResVec::ZERO,
            ResVec::ZERO,
        ),
        0,
    );
    let mut second = sealed_receipt(
        second_key.clone(),
        ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(100), 0),
            ResVec::ZERO,
            ResVec::ZERO,
        ),
        0,
    );
    first.index = u64::MAX;
    second.index = 7;

    let receipts = apply_receipt_transaction(
        &mut ledger,
        Vec::new(),
        vec![vec![second], vec![first]],
        vec![second_key.clone(), first_key.clone()],
    )
    .unwrap();

    assert_eq!(receipts.len(), 2);
    assert_eq!(receipts[0].envelope, first_key);
    assert_eq!(receipts[0].index, 41);
    assert_eq!(receipts[1].envelope, second_key);
    assert_eq!(receipts[1].index, 42);
    assert_eq!(ledger.next_receipt_index(), 43);
}

#[test]
fn duplicate_local_keys_across_workers_roll_back_ledger_and_cursor() {
    let mut ledger = created_ledger();
    let before = ledger.clone();
    let duplicate = receipt(0);

    let result = apply_receipt_transaction(
        &mut ledger,
        Vec::new(),
        vec![vec![duplicate.clone()], vec![duplicate]],
        Vec::new(),
    );

    assert!(result.is_err());
    assert_eq!(ledger, before);
    assert_eq!(ledger.next_receipt_index(), 7);
}

#[test]
fn broken_cross_source_envelope_chain_rolls_back_all_prior_candidates() {
    let envelope_key = key();
    let mut ledger = EnvelopeLedger::new(
        11,
        [Envelope::p3_created(
            envelope_key.clone(),
            Money::from_cents(100),
            0,
            audit_with_remaining(100),
        )],
    )
    .unwrap();
    let before = ledger.clone();
    let mut fill = receipt_from_source(ReceiptFixture {
        key: envelope_key.clone(),
        source: ReceiptSource::SealedIntent(0),
        kind: ReceiptKind::Fill,
        qty_before: 100,
        qty_after: 50,
        value_before: Money::ZERO,
        value_after: Money::from_cents(50),
        delta: ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(50), 0),
            ResVec::ZERO,
            ResVec::new(Money::from_cents(50), 0),
        ),
    });
    fill.deliver_qty = 50;
    let release = receipt_from_source(ReceiptFixture {
        key: envelope_key,
        source: ReceiptSource::SealedIntent(1),
        kind: ReceiptKind::Release,
        qty_before: 49,
        qty_after: 49,
        value_before: Money::from_cents(50),
        value_after: Money::from_cents(50),
        delta: ReceiptDelta::sealed(
            ResVec::ZERO,
            ResVec::new(Money::from_cents(50), 0),
            ResVec::ZERO,
        ),
    });

    let result = apply_receipt_transaction(
        &mut ledger,
        Vec::new(),
        vec![vec![release], vec![fill]],
        Vec::new(),
    );

    assert!(result.is_err());
    assert_eq!(ledger, before);
    assert_eq!(ledger.next_receipt_index(), 11);
}

#[test]
fn illegal_source_kind_and_noncontiguous_local_ordinal_are_transactional() {
    let mut ledger = created_ledger();
    let before = ledger.clone();
    let mut illegal_kind = receipt_from_source(ReceiptFixture {
        key: key(),
        source: ReceiptSource::Auction(0),
        kind: ReceiptKind::Release,
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
    illegal_kind.index = 999;
    assert!(apply_receipt_transaction(
        &mut ledger,
        Vec::new(),
        vec![vec![illegal_kind]],
        Vec::new(),
    )
    .is_err());
    assert_eq!(ledger, before);

    let mut gap = receipt(1);
    gap.index = 999;
    assert!(
        apply_receipt_transaction(&mut ledger, Vec::new(), vec![vec![gap]], Vec::new(),).is_err()
    );
    assert_eq!(ledger, before);
}

#[test]
fn journal_source_mismatch_is_rejected_before_a_worker_receipt_can_exist() {
    let result = ReceiptLocalKey::new(
        JournalRank::PreSeal,
        ReceiptSource::SealedIntent(0),
        ReceiptTransition {
            envelope: key(),
            ordinal: 0,
        },
    );

    assert!(result.is_err());
}

#[test]
fn receipt_index_overflow_rolls_back_ledger_and_cursor() {
    let envelope_key = key();
    let mut ledger = EnvelopeLedger::new(
        u64::MAX,
        [Envelope::p3_created(
            envelope_key.clone(),
            Money::from_cents(100),
            0,
            audit_with_remaining(100),
        )],
    )
    .unwrap();
    let before = ledger.clone();
    let candidate = sealed_receipt(
        envelope_key,
        ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(100), 0),
            ResVec::ZERO,
            ResVec::ZERO,
        ),
        0,
    );

    let result =
        apply_receipt_transaction(&mut ledger, Vec::new(), vec![vec![candidate]], Vec::new());

    assert!(result.is_err());
    assert_eq!(ledger, before);
    assert_eq!(ledger.next_receipt_index(), u64::MAX);
}

#[test]
fn created_envelope_is_inserted_before_its_receipt_and_terminal_removal() {
    let envelope_key = key();
    let created = Envelope::p3_created(
        envelope_key.clone(),
        Money::from_cents(100),
        0,
        audit_with_remaining(100),
    );
    let candidate = sealed_receipt(
        envelope_key.clone(),
        ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(100), 0),
            ResVec::ZERO,
            ResVec::ZERO,
        ),
        0,
    );
    let mut ledger = EnvelopeLedger::new(23, []).unwrap();

    let receipts = apply_receipt_transaction(
        &mut ledger,
        vec![created],
        vec![vec![candidate]],
        vec![envelope_key.clone()],
    )
    .unwrap();

    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].index, 23);
    assert_eq!(ledger.next_receipt_index(), 24);
    assert_eq!(ledger.terminal_count(), 1);
    assert!(ledger.get(&envelope_key).is_err());
}

#[test]
fn duplicate_created_envelope_rolls_back_prior_insertions() {
    let duplicate =
        Envelope::p3_created(key(), Money::from_cents(100), 0, audit_with_remaining(100));
    let unique = Envelope::p3_created(extra().key().clone(), Money::from_cents(1), 0, audit());
    let mut ledger = EnvelopeLedger::new(31, []).unwrap();
    let before = ledger.clone();

    let result = apply_receipt_transaction(
        &mut ledger,
        vec![unique, duplicate.clone(), duplicate],
        Vec::new(),
        Vec::new(),
    );

    assert!(result.is_err());
    assert_eq!(ledger, before);
}

#[test]
fn created_envelope_colliding_with_existing_ledger_rolls_back() {
    let mut ledger = created_ledger();
    let before = ledger.clone();
    let duplicate =
        Envelope::p3_created(key(), Money::from_cents(100), 0, audit_with_remaining(100));

    let result = apply_receipt_transaction(&mut ledger, vec![duplicate], Vec::new(), Vec::new());

    assert!(result.is_err());
    assert_eq!(ledger, before);
}

#[test]
fn non_p3_envelope_is_rejected_from_created_batch_transactionally() {
    let mut ledger = EnvelopeLedger::new(33, []).unwrap();
    let before = ledger.clone();
    let tick_start =
        Envelope::tick_start_existing(key(), Money::from_cents(100), 0, audit_with_remaining(100));

    let result = apply_receipt_transaction(&mut ledger, vec![tick_start], Vec::new(), Vec::new());

    assert!(result.is_err());
    assert_eq!(ledger, before);
}

#[test]
fn unknown_receipt_after_created_insertion_rolls_back_everything() {
    let created = Envelope::p3_created(extra().key().clone(), Money::from_cents(1), 0, audit());
    let unknown = receipt(0);
    let mut ledger = EnvelopeLedger::new(37, []).unwrap();
    let before = ledger.clone();

    let result =
        apply_receipt_transaction(&mut ledger, vec![created], vec![vec![unknown]], Vec::new());

    assert!(result.is_err());
    assert_eq!(ledger, before);
}

#[test]
fn nonzero_terminal_after_valid_receipt_rolls_back_receipt_and_cursor() {
    let envelope_key = key();
    let mut ledger = created_ledger();
    let before = ledger.clone();
    let partial = receipt_from_source(ReceiptFixture {
        key: envelope_key.clone(),
        source: ReceiptSource::SealedIntent(0),
        kind: ReceiptKind::Fill,
        qty_before: 100,
        qty_after: 50,
        value_before: Money::ZERO,
        value_after: Money::from_cents(50),
        delta: ReceiptDelta::sealed(
            ResVec::new(Money::from_cents(50), 0),
            ResVec::ZERO,
            ResVec::new(Money::from_cents(50), 0),
        ),
    });

    let result = apply_receipt_transaction(
        &mut ledger,
        Vec::new(),
        vec![vec![partial]],
        vec![envelope_key],
    );

    assert!(result.is_err());
    assert_eq!(ledger, before);
    assert_eq!(ledger.next_receipt_index(), 7);
}

#[test]
fn missing_terminal_after_valid_receipt_rolls_back_receipt_and_cursor() {
    let mut ledger = created_ledger();
    let before = ledger.clone();

    let result = apply_receipt_transaction(
        &mut ledger,
        Vec::new(),
        vec![vec![receipt(0)]],
        vec![extra().key().clone()],
    );

    assert!(result.is_err());
    assert_eq!(ledger, before);
    assert_eq!(ledger.next_receipt_index(), 7);
}

#[test]
fn omitted_terminal_key_after_full_transition_rolls_back_receipt_and_cursor() {
    let mut ledger = created_ledger();
    let before = ledger.clone();

    let result =
        apply_receipt_transaction(&mut ledger, Vec::new(), vec![vec![receipt(0)]], Vec::new());

    assert!(result.is_err());
    assert_eq!(ledger, before);
    assert_eq!(ledger.next_receipt_index(), 7);
}

#[test]
fn duplicate_terminal_key_rolls_back_the_first_removal() {
    let envelope_key = key();
    let mut ledger = created_ledger();
    let before = ledger.clone();

    let result = apply_receipt_transaction(
        &mut ledger,
        Vec::new(),
        vec![vec![receipt(0)]],
        vec![envelope_key.clone(), envelope_key],
    );

    assert!(result.is_err());
    assert_eq!(ledger, before);
}

#[test]
fn empty_transaction_preserves_receipt_cursor_and_ledger_bytes() {
    let mut ledger = created_ledger();
    let before = ledger.clone();

    let receipts =
        apply_receipt_transaction(&mut ledger, Vec::new(), Vec::new(), Vec::new()).unwrap();

    assert!(receipts.is_empty());
    assert_eq!(ledger, before);
    assert_eq!(ledger.next_receipt_index(), 7);
}
