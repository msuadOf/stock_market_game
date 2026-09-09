import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import type { EngineEvent, Snapshot } from "../types/engine.ts";
import { postWorkerFlush, shouldFlushWorkerEvents } from "./worker-flush.ts";

test("worker stop flushes its tail even while the previous UI frame awaits acknowledgement", () => {
  assert.equal(shouldFlushWorkerEvents(1, true, false), false);
  assert.equal(shouldFlushWorkerEvents(1, true, true), true);
  assert.equal(shouldFlushWorkerEvents(0, true, true), false);
  const workerSource = readFileSync(new URL("./wasm-worker.ts", import.meta.url), "utf8");
  assert.match(workerSource, /function stopLoop\(\)[\s\S]*?flushEvents\(true\);\n\}/);
});

test("worker flush transports events and runtime snapshot as one atomic HostUpdate", () => {
  const sent: unknown[] = [];
  const events = [{ DayBoundary: { seq: 1, day: 1, closed_daily_candles: {} } }] as EngineEvent[];
  const snapshot = { seq: 1, day: 1, tick: 0 } as Snapshot;

  postWorkerFlush({ postMessage: (message) => sent.push(message) }, events, snapshot);

  assert.deepEqual(sent, [
    { type: "hostUpdate", update: {
      type: "delta",
      fromSeq: 1,
      toSeq: 1,
      events,
      runtimeSnapshot: snapshot,
    } },
  ]);
});

test("worker flush omits a runtime snapshot when the batch does not require one", () => {
  const sent: unknown[] = [];
  const events = [{ PriceTick: { seq: 2 } }] as EngineEvent[];

  postWorkerFlush({ postMessage: (message) => sent.push(message) }, events);

  assert.deepEqual(sent, [{ type: "hostUpdate", update: {
    type: "delta",
    fromSeq: 2,
    toSeq: 2,
    events,
    runtimeSnapshot: undefined,
  } }]);
});

test("worker flush preserves raw coverage when fast-forward compaction drops a prefix", () => {
  const sent: unknown[] = [];
  const events = [{ PriceTick: { seq: 60 } }] as EngineEvent[];
  postWorkerFlush({ postMessage: (message) => sent.push(message) }, events, undefined, { fromSeq: 1, toSeq: 60 });
  assert.deepEqual(sent, [{ type: "hostUpdate", update: {
    type: "delta", fromSeq: 1, toSeq: 60, events, runtimeSnapshot: undefined,
  } }]);
});
