use engine::session::protocol::{
    attach_facts, EngineUpdate as ProtocolUpdate, TickBatch, TickFrame,
};
use engine::{Event, Money, StockCode, TradingPhase};
use server::{ClientFrameBuffer, EngineUpdate, MAX_BUFFERED_EVENTS_PER_CLIENT};

fn update(tick: u64, seq: u64) -> EngineUpdate {
    let events = vec![Event::AuctionTick {
        seq,
        tick,
        phase: TradingPhase::ClosingAuction,
        code: StockCode("600101".into()),
        indicative_price: Some(Money::from_cents(1001)),
        matched_volume: 100,
        imbalance: 0,
    }];
    EngineUpdate {
        update: Some(ProtocolUpdate::TickBatch(TickBatch {
            frames: vec![TickFrame {
                tick,
                facts: attach_facts(&events).unwrap(),
                events,
                timeseries_payload: Default::default(),
                seq_from: seq - 1,
                seq_to: seq,
            }],
            runtime_snapshot: None,
        })),
        civil_date: "2030-01-02".into(),
        public_revision: 4,
        timeline_generation: 7,
        failure: None,
    }
}

#[test]
fn publisher_preserves_every_frame_fact_and_metadata_without_compaction() {
    let mut buffer = ClientFrameBuffer::new(120, 0).unwrap();
    let first = update(1, 1);
    let second = update(2, 2);
    buffer.push(first.clone()).unwrap();
    buffer.push(second.clone()).unwrap();

    let published = [buffer.take().unwrap(), buffer.take().unwrap()];

    assert_eq!(
        serde_json::to_value(&published).unwrap(),
        serde_json::to_value([first, second]).unwrap()
    );
    assert!(buffer.take().is_none());
}

#[test]
fn publisher_rejects_malformed_protocol_without_partial_insertion() {
    let mut buffer = ClientFrameBuffer::new(120, 0).unwrap();
    let mut malformed = update(1, 1);
    let Some(ProtocolUpdate::TickBatch(batch)) = &mut malformed.update else {
        panic!()
    };
    batch.frames[0].facts.clear();

    assert!(buffer.push(malformed).is_err());

    assert!(buffer.take().is_none());
}

#[test]
fn publisher_rejects_cursor_gaps_even_after_take() {
    let mut buffer = ClientFrameBuffer::new(120, 0).unwrap();
    buffer.push(update(1, 1)).unwrap();
    buffer.take().unwrap();

    assert!(buffer.push(update(3, 3)).is_err());

    assert!(buffer.take().is_none());
}

#[test]
fn publisher_rejects_generation_changes_without_resync() {
    let mut buffer = ClientFrameBuffer::new(120, 0).unwrap();
    buffer.push(update(1, 1)).unwrap();
    let mut changed = update(2, 2);
    changed.timeline_generation += 1;

    assert!(buffer.push(changed).is_err());
}

#[test]
fn exact_and_mutated_stale_retries_require_resync() {
    for mutated in [false, true] {
        let mut buffer = ClientFrameBuffer::new(120, 0).unwrap();
        let original = update(1, 1);
        buffer.push(original.clone()).unwrap();
        buffer.take().unwrap();
        let mut retry = original;
        if mutated {
            retry.public_revision += 1;
        }

        let result = buffer.push(retry);

        assert!(matches!(
            result,
            Err(server::FrameBufferError::CursorMismatch)
        ));
        assert!(buffer.take().is_none());
    }
}

#[test]
fn snapshot_and_public_metadata_stay_with_their_own_wire_batch() {
    let mut buffer = ClientFrameBuffer::new(120, 0).unwrap();
    let mut first = update(1, 1);
    let Some(ProtocolUpdate::TickBatch(batch)) = &mut first.update else {
        panic!()
    };
    batch.runtime_snapshot = Some(engine::Snapshot {
        seq: 1,
        tick: 1,
        day: 0,
        phase: TradingPhase::ClosingAuction,
        markets: Default::default(),
        accounts: Default::default(),
        daily_candles: Default::default(),
        active_daily_candles: Default::default(),
    });
    let mut second = update(2, 2);
    second.public_revision = 5;
    second.civil_date = "2030-01-03".into();
    buffer.push(first).unwrap();
    buffer.push(second).unwrap();

    let first = serde_json::to_value(buffer.take().unwrap()).unwrap();
    let second = serde_json::to_value(buffer.take().unwrap()).unwrap();

    assert_eq!(first["update"]["TickBatch"]["runtime_snapshot"]["tick"], 1);
    assert!(second["update"]["TickBatch"]["runtime_snapshot"].is_null());
    assert_eq!(first["civil_date"], "2030-01-02");
    assert_eq!(second["civil_date"], "2030-01-03");
    assert_eq!(first["public_revision"], 4);
    assert_eq!(second["public_revision"], 5);
    assert_eq!(first["timeline_generation"], 7);
    assert!(first.get("events").is_none());
}

#[test]
fn publisher_rejects_capacity_without_partial_insertion() {
    let mut buffer = ClientFrameBuffer::new(120, 0).unwrap();
    let mut oversized = update(1, 1);
    let Some(ProtocolUpdate::TickBatch(batch)) = &mut oversized.update else {
        panic!()
    };
    batch.frames = (1..=MAX_BUFFERED_EVENTS_PER_CLIENT + 1)
        .map(|tick| TickFrame {
            tick: u64::try_from(tick).unwrap(),
            events: vec![],
            facts: vec![],
            timeseries_payload: Default::default(),
            seq_from: 0,
            seq_to: 0,
        })
        .collect();

    assert!(matches!(
        buffer.push(oversized),
        Err(server::FrameBufferError::BufferCapacityExceeded { .. })
    ));

    assert!(buffer.take().is_none());
}
