use engine::session::protocol::TickBatch;
use serde_json::{json, Value};

fn wire() -> Value {
    json!({
        "frames": [{
            "tick": 1, "seq_from": 0, "seq_to": 0,
            "facts": [],
            "timeseries_payload": {
                "markets": {}, "active_daily_candles": {}, "closed_daily_candles": {},
                "auction_points": {}, "continuous_points": {}
            }
        }],
        "runtime_snapshot": null,
        "runtime_delta": {
            "tick": 1, "seq_from": 0, "seq_to": 0, "day": 0, "phase": "Continuous",
            "mirror_origin": {
                "authority_epoch": 1,
                "install_sequence": 1,
                "source_tick": 1,
                "source_generation": 1,
                "day_end_stage": "tick",
                "non_tick_revision": 0,
                "mirror_phase": "Continuous"
            },
            "accounts": {},
            "working_orders": { "reset": true, "upserts": [], "removed": [] }
        }
    })
}

#[test]
fn tick_batch_preserves_runtime_delta_on_the_actual_wire() {
    let wire = wire();
    let batch: TickBatch = serde_json::from_value(wire.clone()).unwrap();
    batch.validate().unwrap();
    assert_eq!(
        serde_json::to_value(batch).unwrap()["runtime_delta"],
        wire["runtime_delta"]
    );
}

#[test]
fn tick_batch_rejects_runtime_delta_cursor_mismatch() {
    let mut wire = wire();
    wire["runtime_delta"]["seq_to"] = json!(1);
    let batch: TickBatch = serde_json::from_value(wire).unwrap();
    assert_eq!(
        batch.validate(),
        Err(engine::session::protocol::ProtocolError::RuntimeDeltaMismatch)
    );
}

#[test]
fn tick_batch_rejects_runtime_delta_without_mirror_origin() {
    let mut wire = wire();
    wire["runtime_delta"]
        .as_object_mut()
        .unwrap()
        .remove("mirror_origin");

    assert!(
        serde_json::from_value::<TickBatch>(wire).is_err(),
        "当前 RuntimeDelta 必须显式包含 mirror_origin"
    );
}
