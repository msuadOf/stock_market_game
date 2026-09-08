import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { SaveSlot, Snapshot } from "../types/engine.ts";
import { readWorkerSpeedMetrics, restoreWorkerSlot } from "./worker-host.ts";
import type { WorkerRequestPort } from "./worker-request.ts";

class FakeWorkerPort implements WorkerRequestPort {
  readonly listeners = new Set<(event: MessageEvent) => void>();
  readonly sent: unknown[] = [];

  addEventListener(_type: "message", listener: (event: MessageEvent) => void) {
    this.listeners.add(listener);
  }

  removeEventListener(_type: "message", listener: (event: MessageEvent) => void) {
    this.listeners.delete(listener);
  }

  postMessage(message: unknown) {
    this.sent.push(message);
  }

  emit(data: unknown) {
    for (const listener of this.listeners) listener({ data } as MessageEvent);
  }
}

const slot = {} as SaveSlot;
const snapshot = { tick: 77 } as Snapshot;

describe("worker host restore lifecycle", () => {
  it("keeps a paused worker paused after a successful restore", async () => {
    const worker = new FakeWorkerPort();
    const pending = restoreWorkerSlot(worker, slot, 1, false);

    worker.emit({ type: "restored", requestId: 1, snapshot });

    assert.equal(await pending, snapshot);
    assert.deepEqual(worker.sent, [
      { type: "stop" },
      { type: "restore", requestId: 1, slot },
    ]);
  });

  it("keeps a paused worker paused after a failed restore", async () => {
    const worker = new FakeWorkerPort();
    const pending = restoreWorkerSlot(worker, slot, 2, false);

    worker.emit({ type: "operationError", requestId: 2, message: "存档校验失败" });

    await assert.rejects(pending, /存档校验失败/);
    assert.deepEqual(worker.sent, [
      { type: "stop" },
      { type: "restore", requestId: 2, slot },
    ]);
  });

  it("resumes a running worker after a successful restore", async () => {
    const worker = new FakeWorkerPort();
    const pending = restoreWorkerSlot(worker, slot, 3, true);

    worker.emit({ type: "restored", requestId: 3, snapshot });

    assert.equal(await pending, snapshot);
    assert.deepEqual(worker.sent, [
      { type: "stop" },
      { type: "restore", requestId: 3, slot },
      { type: "start" },
    ]);
  });

  it("resumes a running worker after a failed restore", async () => {
    const worker = new FakeWorkerPort();
    const pending = restoreWorkerSlot(worker, slot, 4, true);

    worker.emit({ type: "operationError", requestId: 4, message: "存档校验失败" });

    await assert.rejects(pending, /存档校验失败/);
    assert.deepEqual(worker.sent, [
      { type: "stop" },
      { type: "restore", requestId: 4, slot },
      { type: "start" },
    ]);
  });
});

describe("worker host speed metrics protocol", () => {
  it("uses the same correlated and validated response shape as every host", async () => {
    const worker = new FakeWorkerPort();
    const pending = readWorkerSpeedMetrics(worker, 5);
    worker.emit({
      type: "speedMetrics",
      requestId: 5,
      metrics: {
        requested: { mode: "fastest" },
        actual_multiplier: 843.25,
        sample_duration_ms: 1_002,
        sample_ticks: 845,
        running: true,
      },
    });

    assert.deepEqual(await pending, {
      requested: { mode: "fastest" },
      actual_multiplier: 843.25,
      sample_duration_ms: 1_002,
      sample_ticks: 845,
      running: true,
    });
    assert.deepEqual(worker.sent, [{ type: "speedMetrics", requestId: 5 }]);
  });

  it("rejects malformed metrics returned across the worker boundary", async () => {
    const worker = new FakeWorkerPort();
    const pending = readWorkerSpeedMetrics(worker, 6);
    worker.emit({
      type: "speedMetrics",
      requestId: 6,
      metrics: {
        requested: { mode: "fixed", multiplier: 60 },
        actual_multiplier: -1,
        sample_duration_ms: 1_000,
        sample_ticks: 60,
        running: true,
      },
    });

    await assert.rejects(pending, /actual_multiplier/);
  });
});
