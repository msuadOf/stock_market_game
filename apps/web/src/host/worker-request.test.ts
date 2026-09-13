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
    const pending = requestWorker(port, { type: "save", requestId: 7, generation: 1 }, "saved");
    port.emit({ type: "saved", requestId: 8, generation: 1, slot: "wrong" });
    port.emit({ type: "saved", requestId: 7, generation: 1, slot: "right" });
    assert.equal((await pending).slot, "right");
    assert.equal(port.listeners.size, 0);
  });

  it("rejects a structured operation error regardless of message wording", async () => {
    const port = new FakePort();
    const pending = requestWorker(port, { type: "restore", requestId: 9, generation: 1 }, "restored");
    port.emit({ type: "operationError", requestId: 9, generation: 1, message: "存档校验失败" });
    await assert.rejects(pending, /存档校验失败/);
    assert.equal(port.listeners.size, 0);
  });

  it("rejects a response from a prior session generation", async () => {
    const port = new FakePort();
    const pending = requestWorker(
      port,
      { type: "publicReports", requestId: 10, generation: 2 },
      "publicReports",
    );
    port.emit({ type: "publicReports", requestId: 10, generation: 1, page: "old" });
    port.emit({ type: "publicReports", requestId: 10, generation: 2, page: "current" });
    assert.equal((await pending).page, "current");
  });

  it("does not let a delayed response resolve a replacement-session request", async () => {
    const port = new FakePort();
    const pending = requestWorker(
      port,
      { type: "publicReportById", requestId: 11, generation: 3, id: "9007199254740993" },
      "publicReportById",
    );
    port.emit({ type: "publicReportById", requestId: 11, generation: 2, report: "old" });
    port.emit({ type: "publicReportById", requestId: 11, generation: 3, report: "current" });
    assert.equal((await pending).report, "current");
  });

  it("does not let a delayed diagnostic response cross a restored generation", async () => {
    const port = new FakePort();
    const pending = requestWorker(
      port,
      { type: "npcDecisionTrace", requestId: 12, generation: 4, account: 1 },
      "npcDecisionTrace",
    );
    port.emit({ type: "npcDecisionTrace", requestId: 12, generation: 3, trace: [{ private: "old" }] });
    port.emit({ type: "npcDecisionTrace", requestId: 12, generation: 4, trace: [] });
    assert.deepEqual((await pending).trace, []);
  });
});
