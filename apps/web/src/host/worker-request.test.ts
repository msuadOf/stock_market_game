import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { requestWorker, type WorkerRequestPort } from "./worker-request.ts";

class FakePort implements WorkerRequestPort {
  listeners = new Set<(event: MessageEvent) => void>();
  sent: unknown[] = [];
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

describe("worker request correlation", () => {
  it("ignores another request response and resolves only its own", async () => {
    const port = new FakePort();
    const pending = requestWorker(port, { type: "save", requestId: 7 }, "saved");
    port.emit({ type: "saved", requestId: 8, slot: "wrong" });
    port.emit({ type: "saved", requestId: 7, slot: "right" });
    assert.equal((await pending).slot, "right");
    assert.equal(port.listeners.size, 0);
  });

  it("rejects a structured operation error regardless of message wording", async () => {
    const port = new FakePort();
    const pending = requestWorker(port, { type: "restore", requestId: 9 }, "restored");
    port.emit({ type: "operationError", requestId: 9, message: "存档校验失败" });
    await assert.rejects(pending, /存档校验失败/);
    assert.equal(port.listeners.size, 0);
  });
});
