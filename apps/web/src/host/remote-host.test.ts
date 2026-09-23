import assert from "node:assert/strict";
import test from "node:test";
import { remoteSpeedValue } from "./remote-host.ts";
import { parseRemoteMessage } from "./remote-wire.ts";
import { remotePausePreferencePayload } from "./remote-request.ts";

const snapshot = {
  seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {},
};

test("Given the remote whole-update DTO, when parsed, then it preserves generation and opaque EngineUpdate", () => {
  const parsed = parseRemoteMessage(JSON.stringify({ PublisherFrame: {
    timeline_generation: 1,
    update: { TickBatch: { frames: [], runtime_snapshot: null } },
    civil_date: "2030-01-02",
    public_revision: 7,
  } }));
  assert.equal(parsed.kind, "protocol");
  if (parsed.kind === "protocol") {
    assert.equal(parsed.update.generation, "1");
    assert.deepEqual(parsed.update.update, { TickBatch: { frames: [], runtime_snapshot: null } });
  }
});

test("Given the remote baseline DTO, when parsed, then it preserves generation and public metadata", () => {
  const parsed = parseRemoteMessage(JSON.stringify({ Baseline: {
    timeline_generation: 2,
    snapshot,
    civil_date: "2030-01-02",
    public_revision: 8,
    public_report_ids: ["7"],
  } }));
  assert.equal(parsed.kind, "baseline");
  if (parsed.kind === "baseline") {
    assert.equal(parsed.update.generation, "2");
    assert.deepEqual(parsed.update.publicPublicationIds, ["7"]);
  }
});

test("Given legacy flat events or frames, when parsed in production, then remote rejects them", () => {
  assert.throws(() => parseRemoteMessage(JSON.stringify({ PriceTick: { seq: 1 } }), "1"), /flat/);
  assert.throws(() => parseRemoteMessage(JSON.stringify({ EngineUpdate: { events: [] } }), "1"), /flat/);
});

test("Given a pause preference payload mode, when converted from speed, then fastest stays JSON safe", () => {
  assert.equal(remoteSpeedValue(Infinity), "Fastest");
  assert.equal(remoteSpeedValue(60), 60);
});

test("Given remote pause preferences, when serialized for the authenticated current generation API, then both flags remain explicit", () => {
  assert.deepEqual(remotePausePreferencePayload("session-1", "2", {
    pause_after_close: true,
    pause_before_open: false,
  }), {
    session_id: "session-1",
    generation: "2",
    preferences: { pause_after_close: true, pause_before_open: false },
  });
});
