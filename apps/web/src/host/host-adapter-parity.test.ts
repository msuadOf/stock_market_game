import assert from "node:assert/strict";
import test from "node:test";
import { createProtocolUpdate } from "./host-update.ts";
import { ProtocolCoordinator } from "./protocol-coordinator.ts";
import { baseState, frame, snapshot, tickBatch } from "./protocol-test-fixtures.ts";

test("Given equivalent WASM Worker remote and Tauri whole updates, when reduced, then every adapter reaches the same protocol state", () => {
  const rawUpdate = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));
  const states = ["wasm", "worker", "remote", "tauri"].map(() => {
    let state = baseState();
    const coordinator = new ProtocolCoordinator({
      onBaseline: (baseline) => { state = baseline; },
      onApplied: (reduction) => { state = reduction.state; },
      onFailure: (failure) => { throw new Error(failure.message); },
    });
    coordinator.accept({ type: "baseline", generation: "1", snapshot: state.snapshot, civilDate: null, revision: null, securities: [], publicPublicationIds: [] });
    coordinator.accept(createProtocolUpdate("1", rawUpdate));
    return state;
  });
  assert.deepEqual(states[0], states[1]);
  assert.deepEqual(states[1], states[2]);
  assert.deepEqual(states[2], states[3]);
});
