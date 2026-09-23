import assert from "node:assert/strict";
import test from "node:test";
import { createTimelineEventGate } from "./tauri-event-coordinator.ts";
import { parseTauriEventPayload, tauriPausePreferenceArgs } from "./tauri-host.ts";

test("Given a restored Tauri timeline, when a previous generation payload arrives, then the gate rejects it", () => {
  const accepted: string[] = [];
  const gate = createTimelineEventGate("old", (value: string) => accepted.push(value));
  assert.equal(gate.accept("old", "first"), true);
  gate.replaceTimeline("new");
  assert.equal(gate.accept("old", "stale"), false);
  assert.equal(gate.accept("new", "fresh"), true);
  assert.deepEqual(accepted, ["first", "fresh"]);
});

test("Given typed Tauri update and pause preferences, when adapted, then old flat events are rejected and command args stay typed", () => {
  assert.deepEqual(parseTauriEventPayload({ session_id: "s", timeline_id: "t", update: { TickBatch: {} } }), {
    session_id: "s", timeline_id: "t", update: { TickBatch: {} },
  });
  assert.throws(() => parseTauriEventPayload({ session_id: "s", timeline_id: "t", events: [] }), /完整协议更新/);
  assert.deepEqual(tauriPausePreferenceArgs("s", { pause_after_close: false, pause_before_open: true }), {
    sessionId: "s", preferences: { pause_after_close: false, pause_before_open: true },
  });
});
