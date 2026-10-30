import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { requestWorker, type WorkerRequestPort } from "./worker-request.ts";
import { CompanyReadRejectedError } from "./company-read-failure.ts";

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

  it("只把当前公司只读请求的typed failure解码并保留原响应", { timeout: 10_000 }, async () => {
    const port = new FakePort();
    const pending = requestWorker(port, { type: "companyReadQuery", requestId: 91, generation: 7, query: { kind: "CompanyCapabilities", data: { company: 3 } } }, "companyReadQuery");
    const response = { type: "operationError", requestId: 91, generation: 7, message: "公司系统错误", failure: { kind: "CorporateState", data: { reason: "权益状态无效" } } };
    port.emit(response);
    await assert.rejects(pending, (error: unknown) => error instanceof CompanyReadRejectedError && error.message === "权益状态无效" && error.cause === response);
  });

  it("公司只读typed failure格式错误时显式携带原响应", { timeout: 10_000 }, async () => {
    const port = new FakePort();
    const pending = requestWorker(port, { type: "companyReadQuery", requestId: 92, generation: 7 }, "companyReadQuery");
    const response = { type: "operationError", requestId: 92, generation: 7, message: "错误", failure: { kind: "UnknownCompany", data: { company: "600101" } } };
    port.emit(response);
    await assert.rejects(pending, (error: unknown) => error instanceof AggregateError && error.cause !== undefined && (error.cause as { response?: unknown }).response === response);
  });

  it("公司只读failure getter抛错时拒绝请求并释放监听器", { timeout: 10_000 }, async () => {
    const port = new FakePort();
    const pending = requestWorker(port, { type: "companyReadQuery", requestId: 93, generation: 7 }, "companyReadQuery");
    const response = { type: "operationError", requestId: 93, generation: 7, message: "错误", get failure(): never { throw new Error("failure getter exploded"); } };
    port.emit(response);
    await assert.rejects(pending, (error: unknown) => error instanceof AggregateError && (error.cause as { response?: unknown }).response === response);
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

  it("keeps concurrent requests correlated when responses arrive in reverse order", async () => {
    const port = new FakePort();
    const first = requestWorker(port, { type: "save", requestId: 13, generation: 4 }, "saved");
    const second = requestWorker(port, { type: "civilDate", requestId: 14, generation: 4 }, "civilDate");
    port.emit({ type: "civilDate", requestId: 14, generation: 4, date: "2030-01-02" });
    port.emit({ type: "saved", requestId: 13, generation: 4, slot: "save" });

    assert.equal((await first).slot, "save");
    assert.equal((await second).date, "2030-01-02");
  });

  it("removes a resolved listener before a duplicate response arrives", async () => {
    const port = new FakePort();
    const pending = requestWorker(port, { type: "save", requestId: 15, generation: 4 }, "saved");
    port.emit({ type: "saved", requestId: 15, generation: 4, slot: "first" });
    assert.equal((await pending).slot, "first");
    port.emit({ type: "saved", requestId: 15, generation: 4, slot: "duplicate" });
    assert.equal(port.listeners.size, 0);
  });
});
