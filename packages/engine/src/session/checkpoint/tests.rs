use super::*;

fn key(generation: u64, epoch: u64, seq: u64, date: u8) -> CheckpointKey {
    CheckpointKey {
        generation,
        writer_epoch: epoch,
        tick: 120,
        seq,
        settled_date: CivilDate::from_ymd(2030, 1, date).unwrap(),
        day: 2,
    }
}

#[test]
fn queue_has_a_fixed_credit_limit_and_one_ordered_receipt_per_commit() {
    let mut order = CheckpointOrder::new(2, 3);
    let first = key(2, 3, 40, 8);
    let second = key(2, 3, 42, 9);
    let mut cursor = second;
    order.staged(first).unwrap();
    order.staged(second).unwrap();
    assert_eq!(order.accept_receipt(second), ReceiptDisposition::OutOfOrder);
    assert_eq!(order.accept_receipt(first), ReceiptDisposition::Accepted);
    assert_eq!(order.accept_receipt(first), ReceiptDisposition::Stale);
    for _ in 1..MAX_PENDING_CHECKPOINTS {
        cursor.seq += 1;
        cursor.settled_date = cursor.settled_date.next().unwrap();
        order.staged(cursor).unwrap();
    }
    cursor.seq += 1;
    cursor.settled_date = cursor.settled_date.next().unwrap();
    assert_eq!(order.can_stage(cursor), Err(CheckpointOrderError::Full));
    assert_eq!(order.accept_receipt(second), ReceiptDisposition::Accepted);
    assert!(order.can_stage(cursor).is_ok());
}

#[test]
fn writer_swap_discards_old_receipts_without_consuming_new_credit() {
    let mut order = CheckpointOrder::new(2, 3);
    let old = key(2, 3, 100, 10);
    order.staged(old).unwrap();
    order.install(2, 3);
    assert_eq!(
        order.can_stage(old),
        Err(CheckpointOrderError::NonIncreasing)
    );
    order.install(3, 4);
    let current = key(3, 4, 12, 2);
    order.staged(current).unwrap();
    assert_eq!(order.accept_receipt(old), ReceiptDisposition::Stale);
    assert_eq!(
        order.can_stage(old),
        Err(CheckpointOrderError::WrongBinding)
    );
    assert_eq!(order.accept_receipt(current), ReceiptDisposition::Accepted);
}

#[test]
fn capture_is_immutable_exact_and_usable_without_observer_ack() {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = CivilDate::from_iso("2030-01-05").unwrap();
    let mut session = ProtocolSession::new(setup, 77).unwrap();
    let first = session.end_civil_day_update().unwrap();
    let first_capture = CapturedCheckpoint::take(&session, &first, 1, 3).unwrap();
    assert!(CapturedCheckpoint::take(&session, &first, 1, 3).is_err());
    let latest = session.save().unwrap();
    assert_eq!(first_capture.slot.snapshot.seq, latest.snapshot.seq);
    assert_eq!(
        first_capture.key.settled_date,
        latest.civil_clock.settled_through.unwrap()
    );
    let second = session.end_civil_day_update().unwrap();
    let next_capture = CapturedCheckpoint::take(&session, &second, 1, 3).unwrap();
    assert_ne!(next_capture.key, first_capture.key);
    assert_eq!(next_capture.slot.snapshot.seq, second.seq_to);
    assert_eq!(first_capture.slot.snapshot.seq, first.seq_to);
    assert_ne!(
        first_capture.slot.civil_clock.settled_through,
        next_capture.slot.civil_clock.settled_through
    );
}

#[test]
fn worker_sequence_receipt_only_releases_its_exact_head() {
    use ReceiptDisposition::{Accepted, OutOfOrder, Stale};
    let mut order = CheckpointOrder::new(2, 3);
    let first = key(2, 3, 40, 8);
    let second = key(2, 3, 42, 9);
    order.staged(first).unwrap();
    order.staged(second).unwrap();
    assert_eq!(order.accept_pending_seq(2, 2, 40), Stale);
    assert_eq!(order.accept_pending_seq(3, 3, 40), Stale);
    assert_eq!(order.accept_pending_seq(2, 3, 42), OutOfOrder);
    assert_eq!(order.accept_pending_seq(2, 3, 41), OutOfOrder);
    assert_eq!(order.accept_pending_seq(2, 3, 40), Accepted);
    assert_eq!(order.accept_pending_seq(2, 3, 40), Stale);
    assert_eq!(order.accept_pending_seq(2, 3, 42), Accepted);
    assert_eq!(order.accept_pending_seq(2, 3, 43), OutOfOrder);
}
