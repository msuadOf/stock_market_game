use engine::session::protocol::{TickBatch, TickFrame, TickTimeseriesPayload};
use engine::{AccountId, Event, StockCode};

fn frame(tick: u64, before: u64, count: u64) -> TickFrame {
    let mut frame = TickFrame {
        facts: Vec::new(),
        tick,
        seq_from: before,
        seq_to: before + count,
        events: (before + 1..=before + count)
            .map(|seq| Event::IntentRejected {
                seq,
                account: AccountId(0),
                code: StockCode("600001".into()),
                reason: engine::RejectionReason::UnknownStock,
            })
            .collect(),
        timeseries_payload: TickTimeseriesPayload::default(),
    };
    frame.facts = engine::session::protocol::attach_facts(&frame.events).unwrap();
    frame
}

#[test]
fn replay_rejects_mutated_payload_but_accepts_reversed_facts() {
    use engine::session::protocol::{EngineUpdate, ReplayDecision, ReplayGuard};
    let original = frame(1, 0, 3);
    let mut guard = ReplayGuard::new(0, 0);
    let update = |frame| {
        EngineUpdate::TickBatch(TickBatch {
            frames: vec![frame],
            runtime_snapshot: None,
        })
    };
    assert_eq!(
        guard.ingest(&update(original.clone())).unwrap(),
        ReplayDecision::Applied
    );
    let mut reversed = original.clone();
    reversed.events.reverse();
    reversed.facts.reverse();
    assert_eq!(
        guard.ingest(&update(reversed)).unwrap(),
        ReplayDecision::ExactRetry
    );
    let mut mutated = original.clone();
    mutated.facts[0].canonical_payload.push(' ');
    assert!(guard.ingest(&update(mutated)).is_err());
    let mut duplicate = original;
    duplicate.facts[1].key = duplicate.facts[0].key.clone();
    assert!(duplicate.validate().is_err());
}

#[test]
fn empty_frame_carries_unchanged_exclusive_cursor() {
    let empty = frame(4, 12, 0);
    assert!(empty.validate().is_ok());
    assert_eq!((empty.seq_from, empty.seq_to), (12, 12));
}

#[test]
fn event_permutation_preserves_frame_validation_and_timeseries() {
    let mut reordered = frame(1, 0, 3);
    let timeseries = serde_json::to_value(&reordered.timeseries_payload).unwrap();
    reordered.events.reverse();
    assert!(reordered.validate().is_ok());
    assert_eq!(
        serde_json::to_value(&reordered.timeseries_payload).unwrap(),
        timeseries
    );
}

#[test]
fn missing_duplicate_and_outside_sequence_are_rejected() {
    let original = frame(1, 0, 3);
    let mut missing = original.clone();
    missing.events.pop();
    assert!(missing.validate().is_err());
    let mut duplicate = original.clone();
    duplicate.events[1] = duplicate.events[0].clone();
    assert!(duplicate.validate().is_err());
    let mut outside = original;
    outside.seq_from = 1;
    outside.seq_to = 4;
    assert!(outside.validate().is_err());
}

#[test]
fn batches_preserve_all_frames_including_empty_ticks() {
    let frames = vec![frame(1, 0, 2), frame(2, 2, 0), frame(3, 2, 1)];
    let batch = TickBatch {
        frames,
        runtime_snapshot: None,
    };
    assert!(batch.validate().is_ok());
    assert_eq!(batch.frames.len(), 3);
    assert_eq!(batch.frames[1].tick, 2);
}

#[test]
fn batches_reject_tick_reorder_duplicate_gap_and_sequence_gap() {
    for frames in [
        vec![frame(2, 0, 1), frame(1, 1, 1)],
        vec![frame(1, 0, 1), frame(1, 1, 1)],
        vec![frame(1, 0, 1), frame(3, 1, 1)],
        vec![frame(1, 0, 1), frame(2, 2, 1)],
        vec![],
    ] {
        assert!(TickBatch {
            frames,
            runtime_snapshot: None
        }
        .validate()
        .is_err());
    }
}

#[test]
fn final_snapshot_must_match_both_frame_cursors() {
    let snapshot = engine::Snapshot {
        seq: 1,
        tick: 1,
        day: 0,
        phase: engine::TradingPhase::Continuous,
        markets: Default::default(),
        accounts: Default::default(),
        daily_candles: Default::default(),
        active_daily_candles: Default::default(),
    };
    let mut batch = TickBatch {
        frames: vec![frame(1, 0, 1)],
        runtime_snapshot: Some(snapshot.clone()),
    };
    assert!(batch.validate().is_ok());
    batch.runtime_snapshot = Some(engine::Snapshot {
        tick: 2,
        ..snapshot.clone()
    });
    assert!(batch.validate().is_err());
    batch.runtime_snapshot = Some(engine::Snapshot { seq: 2, ..snapshot });
    assert!(batch.validate().is_err());
}

#[test]
fn wire_rejects_integers_beyond_javascript_safe_range() {
    let mut oversized = frame(1, 0, 0);
    oversized.tick = 9_007_199_254_740_992;
    assert!(serde_json::to_value(&oversized).is_err());
    let valid = serde_json::to_value(frame(1, 0, 0)).unwrap();
    let mut malicious = valid;
    malicious["seq_to"] = serde_json::json!(9_007_199_254_740_992_u64);
    assert!(serde_json::from_value::<TickFrame>(malicious).is_err());
}
