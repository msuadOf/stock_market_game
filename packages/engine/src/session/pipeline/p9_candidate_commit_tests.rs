use super::ledger_tests::{audit_with_remaining, created_ledger, expiry_release, key, second_key};
use super::p9_candidate_commit::prepare_p9_candidate_commit;
use super::{
    Envelope, EnvelopeAudit, EnvelopeLedger, EnvelopeOrigin, FeeComponents, ResVec, StepFatal,
};
use crate::{GameSession, Money};

fn game() -> GameSession {
    GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap()
}

fn ledger_with_live_audit_and_tick_local_evidence() -> (EnvelopeLedger, EnvelopeAudit) {
    let live_key = second_key();
    let terminal_key = key();
    let live_audit = EnvelopeAudit {
        limit: Money::from_cents(1_000),
        remaining_qty: 2,
        filled_qty: 1,
        filled_value: Money::from_cents(100),
        nominal: FeeComponents {
            commission: Money::from_cents(500),
            stamp_tax: Money::from_cents(10),
            transfer_fee: Money::from_cents(1),
        },
        charged: FeeComponents {
            commission: Money::from_cents(100),
            stamp_tax: Money::ZERO,
            transfer_fee: Money::ZERO,
        },
    };
    let mut ledger = EnvelopeLedger::new(
        7,
        [
            Envelope::p3_created(live_key, Money::ZERO, 2, live_audit),
            Envelope::tick_start_existing(
                terminal_key.clone(),
                Money::from_cents(100),
                0,
                audit_with_remaining(0),
            ),
        ],
    )
    .unwrap();
    let mut terminal = expiry_release(terminal_key.clone(), ResVec::new(Money::from_cents(100), 0));
    ledger.apply(std::slice::from_mut(&mut terminal)).unwrap();
    ledger.remove_terminal(&[terminal_key]).unwrap();
    (ledger, live_audit)
}

#[test]
fn prepared_p9_rebases_live_ledger_then_commits_without_a_fallible_tail() {
    let mut authority = game();
    let mut candidate = authority.clone_for_tick_shadow().unwrap();
    let (ledger, audit) = ledger_with_live_audit_and_tick_local_evidence();
    candidate.envelope_ledger = ledger;
    candidate.next_receipt_base = 8;
    candidate.seq = candidate.seq.checked_add(3).unwrap();

    let prepared = prepare_p9_candidate_commit(&mut authority, candidate).unwrap();
    let committed = prepared.commit();

    assert_eq!(
        authority.business_state_hash().unwrap(),
        committed.business_hash()
    );
    assert_eq!(
        authority.session_state_hash().unwrap(),
        committed.session_hash()
    );
    assert_eq!(committed.next_receipt_base(), 8);
    assert_eq!(authority.next_receipt_base, 8);
    assert_eq!(authority.envelope_ledger.next_receipt_index(), 8);
    assert_eq!(authority.envelope_ledger.terminal_count(), 0);
    assert!(authority.envelope_ledger.seen_local_keys.is_empty());
    assert_eq!(authority.envelope_ledger.audits.len(), 1);
    assert_eq!(authority.envelope_ledger.conservation.len(), 1);
    let live = authority.envelope_ledger.get(&second_key()).unwrap();
    assert_eq!(live.origin(), EnvelopeOrigin::TickStart);
    assert_eq!(live.audit(), audit);
    assert_eq!(live.basis(), ResVec::new(Money::ZERO, 2));
    assert_eq!(live.spent(), ResVec::ZERO);
    assert_eq!(live.released(), ResVec::ZERO);
}

#[test]
fn rebase_failure_discards_the_candidate_without_touching_authority() {
    let mut authority = game();
    let mut candidate = authority.clone_for_tick_shadow().unwrap();
    candidate.envelope_ledger = created_ledger();
    candidate.next_receipt_base = candidate.envelope_ledger.next_receipt_index();
    candidate
        .envelope_ledger
        .audits
        .get_mut(&key())
        .unwrap()
        .remaining_qty = 99;
    let business_before = authority.business_state_hash().unwrap();
    let session_before = authority.session_state_hash().unwrap();

    assert!(prepare_p9_candidate_commit(&mut authority, candidate).is_err());
    assert_eq!(authority.business_state_hash().unwrap(), business_before);
    assert_eq!(authority.session_state_hash().unwrap(), session_before);
}

#[test]
fn dropping_a_prepared_candidate_does_not_change_authority() {
    let mut authority = game();
    let business_before = authority.business_state_hash().unwrap();
    let session_before = authority.session_state_hash().unwrap();
    let mut candidate = authority.clone_for_tick_shadow().unwrap();
    candidate.seq = candidate.seq.checked_add(1).unwrap();

    let prepared = prepare_p9_candidate_commit(&mut authority, candidate).unwrap();
    drop(prepared);
    assert_eq!(authority.business_state_hash().unwrap(), business_before);
    assert_eq!(authority.session_state_hash().unwrap(), session_before);
}

#[test]
fn split_candidate_receipt_cursor_is_rejected_before_commit() {
    let mut authority = game();
    let mut candidate = authority.clone_for_tick_shadow().unwrap();
    candidate.next_receipt_base = 1;
    let business_before = authority.business_state_hash().unwrap();
    let session_before = authority.session_state_hash().unwrap();

    assert!(matches!(
        prepare_p9_candidate_commit(&mut authority, candidate),
        Err(StepFatal::InvariantViolation { location, .. })
            if location == "pipeline::p9_candidate_commit"
    ));
    assert_eq!(authority.business_state_hash().unwrap(), business_before);
    assert_eq!(authority.session_state_hash().unwrap(), session_before);
}
