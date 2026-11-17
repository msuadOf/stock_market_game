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

fn captured_days() -> (ProtocolSession, CivilUpdate, CapturedCheckpoint) {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = CivilDate::from_iso("2030-01-05").unwrap();
    let mut session = ProtocolSession::new(setup, 77).unwrap();
    let update = session.end_civil_day_update().unwrap();
    let capture = CapturedCheckpoint::take(&session, &update, 1, 1).unwrap();
    (session, update, capture)
}

#[test]
fn archive_rejection_retains_sealed_candidate_and_blocks_next_boundary() {
    let (_, _, capture) = captured_days();
    let key = capture.key;
    let slot = Arc::clone(&capture.slot);
    let mut authority = ArchiveAuthority::new(1, 1);
    assert!(authority
        .stage_captured(capture, |_| Err("队列满".into()))
        .is_err());
    assert!(authority.is_blocked());
    assert_eq!(authority.durable(), None);
    assert_eq!(authority.pending_count(), 1);
    assert_eq!(authority.retained.front().unwrap().key, key);
    assert!(Arc::ptr_eq(
        &authority.retained.front().unwrap().slot,
        &slot
    ));
    assert!(authority.ensure_drained().is_err());
}

#[test]
fn archive_failed_receipt_does_not_release_sealed_checkpoint_or_mark_durable() {
    let (_, _, capture) = captured_days();
    let key = capture.key;
    let mut authority = ArchiveAuthority::new(1, 1);
    authority.stage_captured(capture, |_| Ok(())).unwrap();
    assert!(matches!(
        authority.complete(key, Err("写盘失败".into())),
        ReceiptOutcome::Failed { .. }
    ));
    assert_eq!(authority.pending_count(), 1);
    assert_eq!(authority.durable(), None);
    assert!(authority.is_blocked());
    // 同一失败不能被迟到的成功包解锁；必须明确建立新会话。
    assert!(matches!(
        authority.complete(key, Ok(())),
        ReceiptOutcome::Failed { .. }
    ));
    assert_eq!(authority.durable(), None);
}

#[test]
fn archive_receipts_distinguish_stale_bad_and_durable() {
    let (_, _, capture) = captured_days();
    let key = capture.key;
    let mut authority = ArchiveAuthority::new(1, 1);
    authority.stage_captured(capture, |_| Ok(())).unwrap();
    let mut stale = key;
    stale.writer_epoch = 0;
    assert!(matches!(
        authority.complete(stale, Err("旧 writer".into())),
        ReceiptOutcome::Stale
    ));
    assert!(!authority.is_blocked());
    assert!(matches!(
        authority.complete(key, Ok(())),
        ReceiptOutcome::Durable
    ));
    assert_eq!(authority.durable(), Some(key));
    assert!(authority.ensure_drained().is_ok());
    assert!(matches!(
        authority.complete(key, Ok(())),
        ReceiptOutcome::Stale
    ));
    let mut bad = key;
    bad.tick += 1;
    assert!(matches!(
        authority.complete(bad, Ok(())),
        ReceiptOutcome::Failed { .. }
    ));
    assert!(authority.is_blocked());
}

#[test]
fn archive_replacement_fences_old_receipts_and_releases_old_blocked_state() {
    let (_, _, capture) = captured_days();
    let key = capture.key;
    let mut authority = ArchiveAuthority::new(1, 1);
    authority.stage_captured(capture, |_| Ok(())).unwrap();
    authority.complete(key, Err("写盘失败".into()));
    authority.install(2, 2);
    assert!(matches!(
        authority.complete(key, Ok(())),
        ReceiptOutcome::Stale
    ));
    assert!(!authority.is_blocked());
    assert_eq!(authority.pending_count(), 0);
    assert_eq!(authority.durable(), None);
}

#[test]
fn storage_fence_stops_later_writes_after_first_failure_until_new_binding() {
    let mut fence = ArchiveWriteFence::default();
    let first = key(2, 3, 40, 8);
    let second = key(2, 3, 42, 9);
    assert!(fence.begin(first).is_ok());
    fence.finish(first, &Err("磁盘故障".into()));
    assert!(fence.begin(second).is_err());
    assert!(fence.ensure_finished().is_err());
    let replacement = key(3, 4, 10, 2);
    assert!(fence.begin(replacement).is_ok());
    fence.finish(replacement, &Ok(()));
    assert!(fence.ensure_finished().is_ok());
    assert!(fence.begin(second).is_err());
}

#[test]
fn earlier_success_remains_durable_when_a_later_enqueue_is_blocked() {
    let (mut session, _, first) = captured_days();
    let first_key = first.key;
    let mut authority = ArchiveAuthority::new(1, 1);
    authority.stage_captured(first, |_| Ok(())).unwrap();
    let update = session.end_civil_day_update().unwrap();
    assert!(authority
        .stage(&session, &update, |_| Err("队列满".into()))
        .is_err());
    assert!(matches!(
        authority.complete(first_key, Ok(())),
        ReceiptOutcome::Durable
    ));
    assert_eq!(authority.durable(), Some(first_key));
    assert_eq!(authority.pending_count(), 1);
    assert!(authority.is_blocked());
    assert!(authority.ensure_drained().is_err());
}
