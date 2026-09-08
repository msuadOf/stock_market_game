import assert from "node:assert/strict";
import test from "node:test";
import type { EngineEvent, Snapshot } from "../types/engine.ts";
import { postWorkerFlush } from "./worker-flush.ts";

test("worker flush delivers events before the runtime snapshot they precede", () => {
  const sent: unknown[] = [];
  const events = [{ DayBoundary: { day: 1 } }] as unknown as EngineEvent[];
  const snapshot = { day: 1, tick: 0 } as Snapshot;

  postWorkerFlush({ postMessage: (message) => sent.push(message) }, events, snapshot);

  assert.deepEqual(sent, [
    { type: "events", events },
    { type: "snapshot", snapshot },
  ]);
});

test("worker flush omits a runtime snapshot when the batch does not require one", () => {
  const sent: unknown[] = [];
  const events = [] as EngineEvent[];

  postWorkerFlush({ postMessage: (message) => sent.push(message) }, events);

  assert.deepEqual(sent, [{ type: "events", events }]);
});
