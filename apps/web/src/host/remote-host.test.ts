import assert from "node:assert/strict";
import test from "node:test";
import type { SessionSetup } from "../types/engine.ts";
import type { HostFailure } from "./host-update.ts";
import { createRemoteHost, remoteSpeedValue } from "./remote-host.ts";
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

test("Given remote session deletion fails, when disposed, then the host reports an explicit fatal failure", async () => {
  const failures: HostFailure[] = [];
  const fetchFn = (async (input: string | URL | Request, init?: RequestInit) => {
    const url = input instanceof Request ? input.url : String(input);
    if (url.endsWith("/api/new")) {
      return new Response(JSON.stringify({ session_id: "session-1", session_token: "token-1" }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    }
    if (init?.method === "DELETE") return new Response("delete denied", { status: 500 });
    return new Response(null, { status: 200 });
  }) as typeof fetch;
  const socket = {
    readyState: 1,
    close() {},
    send() {},
    onmessage: null,
    onerror: null,
    onclose: null,
  } as unknown as WebSocket;
  const remote = await createRemoteHost({} as SessionSetup, 1n, {
    baseUrl: "http://127.0.0.1:3000",
    fetchFn,
    webSocketFactory: () => socket,
  });
  remote.start(() => {}, (failure) => failures.push(failure));
  remote.dispose();
  await new Promise((resolve) => setImmediate(resolve));
  assert.deepEqual(failures, [{
    code: "REMOTE_DISPOSE",
    where: "remote-host.dispose",
    message: "远程服务请求失败（HTTP 500）：delete denied",
  }]);
});
