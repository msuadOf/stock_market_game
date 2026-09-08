import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { SaveSlot, Snapshot } from "../types/engine.ts";
import { restoreWorkerSlot } from "./worker-host.ts";
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
