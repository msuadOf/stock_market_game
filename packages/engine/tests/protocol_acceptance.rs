use engine::session::protocol::*;
use engine::{AccountId, Event, Money, StockId, TradingPhase};

fn events() -> Vec<Event> {
    (1..=3)
        .map(|seq| Event::IntentRejected {
            seq,
            account: AccountId(0),
            code: StockId(0),
            reason: engine::RejectionReason::UnknownStock,
        })
        .collect()
}

/// 独立测试 fixture 的 producer：事实身份在此一次分配。
fn fixture_facts(events: &[Event]) -> Vec<EventFact> {
    SessionFactCursor::default()
        .assign_session_facts(events.to_vec())
        .unwrap()
}

fn frame() -> TickFrame {
    TickFrame {
        tick: 1,
        facts: fixture_facts(&events()),
        timeseries_payload: Default::default(),
        seq_from: 0,
        seq_to: 3,
    }
}

fn update(frame: TickFrame) -> EngineUpdate {
    EngineUpdate::TickBatch(
        TickBatch {
            frames: vec![frame],
            runtime_snapshot: None,
            runtime_delta: None,
        }
        .into(),
    )
}

#[test]
fn tick_update_retains_the_json_contract_across_native_layout_changes() {
    let frame = frame();
    let expected = serde_json::json!({ "TickBatch": {
        "frames": [serde_json::to_value(&frame).unwrap()],
        "runtime_snapshot": null,
    }});
    assert_eq!(serde_json::to_value(update(frame)).unwrap(), expected);
    let restored: EngineUpdate = serde_json::from_value(expected.clone()).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), expected);
}

#[test]
fn optional_imbalance_rejects_unsafe_wire_values() {
    let event = Event::AuctionTick {
        seq: 1,
        tick: 1,
        phase: TradingPhase::CallAuction,
        code: StockId(0),
        indicative_price: Some(Money::from_cents(1000)),
        matched_volume: 100,
        imbalance: 0,
    };
    let key = fixture_facts(&[event]).remove(0).key;
    let mut point = AuctionPoint {
        key,
        tick: 1,
        kind: AuctionPointKind::Indication,
        phase: TradingPhase::CallAuction,
        indicative_price: None,
        matched_volume: 100,
        imbalance: None,
    };
    for value in [None, Some(0), Some(9_007_199_254_740_991)] {
        point.imbalance = value;
        let wire = serde_json::to_value(&point).unwrap();
        assert_eq!(
            serde_json::from_value::<AuctionPoint>(wire)
                .unwrap()
                .imbalance,
            value
        );
    }
    let mut wire = serde_json::to_value(&point).unwrap();
    wire["imbalance"] = serde_json::json!(9_007_199_254_740_992_u64);
    assert!(serde_json::from_value::<AuctionPoint>(wire).is_err());
    point.imbalance = Some(9_007_199_254_740_992);
    assert!(serde_json::to_value(point).is_err());
}

#[test]
fn fact_correspondence_preserves_multiplicity_and_rejects_substitutions() {
    let original = frame();
    assert_eq!(original.facts.len(), 3);
    original.validate().unwrap();
    let mut reversed = original.clone();
    reversed.facts.reverse();
    reversed.validate().unwrap();
    for variant in 0..5 {
        let mut malformed = original.clone();
        match variant {
            0 => {
                malformed.facts.pop();
            }
            1 => {
                malformed.facts[1] = malformed.facts[0].clone();
            }
            2 => {
                // 同一事实记录整体重复：seq 重复即身份失败
                malformed.facts[1] = malformed.facts[0].clone();
                malformed.facts[1].key = engine::session::pipeline::EventStableKey::for_event(
                    &malformed.facts[0].event,
                    1,
                );
            }
            3 => {
                malformed.facts[0].key = engine::session::pipeline::EventStableKey::for_event(
                    &Event::IntentRejected {
                        seq: 1,
                        account: AccountId(99),
                        code: StockId(0),
                        reason: engine::RejectionReason::UnknownStock,
                    },
                    0,
                );
            }
            // 载荷篡改：改动事件字段使 canonical 内容与首次发布不一致（原 canonical_payload 负例）
            4 => {
                if let Event::IntentRejected { account, .. } = &mut malformed.facts[0].event {
                    *account = AccountId(9);
                }
            }
            _ => unreachable!(),
        }
        assert!(malformed.validate().is_err(), "variant {variant}");
    }
}

#[test]
fn coherent_mutation_is_replay_mismatch_not_identity_failure() {
    let original = frame();
    let mut guard = ReplayGuard::new(0, 0);
    assert_eq!(
        guard.ingest(&update(original.clone())).unwrap(),
        ReplayDecision::Applied
    );
    let mut reordered = original.clone();
    reordered.facts.reverse();
    assert_eq!(
        guard.ingest(&update(reordered)).unwrap(),
        ReplayDecision::ExactRetry
    );
    let mut changed = original;
    if let Event::IntentRejected { code, .. } = &mut changed.facts[0].event {
        *code = StockId(1);
        changed.facts[0].key = engine::session::pipeline::EventStableKey::for_event(
            &changed.facts[0].event,
            changed.facts[0].key.local_event_index(),
        );
    }
    changed.validate().unwrap();
    assert_eq!(
        guard.ingest(&update(changed)),
        Err(ProtocolError::ReplayMismatch)
    );
}

#[test]
fn starting_context_rejects_stale_future_and_sequence_gaps() {
    for (tick, seq) in [(1, 0), (2, 3), (0, 1)] {
        assert_eq!(
            ReplayGuard::new(tick, seq).ingest(&update(frame())),
            Err(ProtocolError::ReplayMismatch)
        );
    }
}

#[test]
fn equal_sets_with_different_multiplicity_are_rejected() {
    let mut malformed = frame();
    malformed.facts[2] = malformed.facts[1].clone();
    malformed.facts[2].key =
        engine::session::pipeline::EventStableKey::for_event(&malformed.facts[2].event, 2);
    assert_eq!(malformed.validate(), Err(ProtocolError::FactIdentity));
}
