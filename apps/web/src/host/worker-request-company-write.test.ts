import assert from "node:assert/strict";
import test from "node:test";
import { WorkerRequestScope, type WorkerRequestPort } from "./worker-request.ts";
import { CompanyWriteRejectedError } from "./company-write.ts";

class Port implements WorkerRequestPort {
  readonly listeners = new Set<(event: MessageEvent) => void>();
  addEventListener(_type: "message", listener: (event: MessageEvent) => void) { this.listeners.add(listener); }
  removeEventListener(_type: "message", listener: (event: MessageEvent) => void) { this.listeners.delete(listener); }
  postMessage(_message: unknown) {}
  emit(data: unknown) { for (const listener of this.listeners) listener({ data } as MessageEvent); }
}

test("三写 scope 严格处理 own failure 与 message-only，协议错仍清理当前请求", { timeout: 10000 }, async () => {
  const port = new Port();
  const scope = new WorkerRequestScope(port);
  try {
    for (const fields of [
      { failure: undefined }, { failure: null }, { failure: { kind: "PlayerProposal", data: {} } },
      { failure: cases[2].failure }, {}, { message: "" }, { message: " \t " }, { message: 17 },
    ]) {
      const requestId = scope.nextRequestId();
      const pending = scope.request({ type: "proposePlayerCompanyAction", requestId, generation: 1 }, "unused");
      const response = { type: "operationError", requestId, generation: 1, message: "具体原因", ...fields };
      if (Object.keys(fields).length === 0) Reflect.deleteProperty(response, "message");
      const rejected = assert.rejects(pending, error => {
        assert.ok(error instanceof Error);
        assert.match(error.message, /协议/);
        assert.equal(error instanceof CompanyWriteRejectedError, false);
        assert.ok(error.cause);
        return true;
      });
      port.emit(response);
      await rejected;
      assert.equal(scope.pendingCount(), 0);
      assert.equal(port.listeners.size, 0);
    }
    const requestId = scope.nextRequestId();
    const pending = scope.request({ type: "cancelReportCorrection", requestId, generation: 1 }, "unused");
    const response = { type: "operationError", requestId, generation: 1, message: "当前 WASM bindings 不支持取消" };
    const rejected = assert.rejects(pending, error => {
      assert.ok(error instanceof Error);
      assert.equal(error instanceof CompanyWriteRejectedError, false);
      assert.equal(error.message, response.message);
      assert.equal(error.cause, response);
      return true;
    });
    port.emit(response);
    await rejected;
  } finally { scope.close(new Error("测试清理")); }
});

test("并发公司写一项业务拒绝只释放本人资源，其他在途请求仍成功", { timeout: 10000 }, async () => {
  const port = new Port(), scope = new WorkerRequestScope(port);
  try {
    const firstId = scope.nextRequestId(), secondId = scope.nextRequestId();
    const first = scope.request({ type: "proposePlayerCompanyAction", requestId: firstId, generation: 1 }, "playerCompanyActionAcknowledged");
    const rejected = assert.rejects(first, CompanyWriteRejectedError);
    const second = scope.request({ type: "cancelReportCorrection", requestId: secondId, generation: 1 }, "reportCorrectionCancelAcknowledged");
    port.emit({ type: "operationError", requestId: firstId, generation: 1, message: "具体原因", failure: cases[0].failure });
    await rejected;
    assert.equal(scope.pendingCount(), 1);
    assert.equal(port.listeners.size, 1);
    port.emit({ type: "reportCorrectionCancelAcknowledged", requestId: secondId, generation: 1, result: { operation_id: "传输检查" } });
    assert.deepEqual((await second).result, { operation_id: "传输检查" });
    assert.equal(scope.pendingCount(), 0);
    assert.equal(port.listeners.size, 0);
  } finally { scope.close(new Error("测试清理")); }
});

// synthetic transport fixture 仅检验公开 failure 的传输，不能充当 Rust producer 或领域执行证据。
const cases = [
  { type: "proposePlayerCompanyAction", failure: { kind: "PlayerProposal", data: { account: "0", company: 0, kind: "CashDividend", day: "2030-01-02", classification: "BusinessCondition", reason: "本人真实持仓不足" } } },
  { type: "enqueueReportCorrection", failure: { kind: "ReportCorrection", data: { kind: "OperationConflict", data: { operation_id: "传输检查", completed: false } } } },
  { type: "cancelReportCorrection", failure: { kind: "ReportCorrection", data: { kind: "NotPending", data: { operation_id: "传输检查" } } } },
] as const;

for (const row of cases) {
  test(`WorkerRequestScope ${row.type} 匹配拒绝保完整公开 failure 与 cause`, { timeout: 10000 }, async () => {
    const port = new Port();
    const scope = new WorkerRequestScope(port);
    try {
      const requestId = scope.nextRequestId();
      const result = scope.request({ type: row.type, requestId, generation: 1 }, "未使用的成功响应")
        .then(value => ({ ok: true as const, value }), (error: unknown) => ({ ok: false as const, error }));
      port.emit({ type: "operationError", requestId: requestId + 1, generation: 1, failure: null });
      port.emit({ type: "operationError", requestId, generation: 2, failure: null });
      assert.equal(scope.pendingCount(), 1, "不匹配响应不能触发 failure decoder 或清理当前请求");
      port.emit({ type: "operationError", requestId, generation: 1, message: "公司命令的具体业务拒绝", failure: row.failure });
      const outcome = await result;
      assert.equal(outcome.ok, false);
      if (outcome.ok) throw new Error("业务拒绝不能成功");
      assert.ok(outcome.error instanceof Error);
      assert.equal(scope.pendingCount(), 0);
      assert.equal(port.listeners.size, 0);
      const nextId = scope.nextRequestId();
      const next = scope.request({ type: "save", requestId: nextId, generation: 1 }, "saved");
      port.emit({ type: "saved", requestId: nextId, generation: 1, slot: "正控存档" });
      assert.equal((await next).slot, "正控存档", "一项拒绝不得关闭 scope");
      assert.equal(scope.pendingCount(), 0);
      assert.equal(port.listeners.size, 0);
      assert.deepEqual(outcome.error.cause, row.failure, "拒绝 Error.cause 必须保完整公开 typed failure");
      assert.deepEqual(Reflect.get(outcome.error, "failure"), row.failure, "拒绝 Error 必须保可读取的公开 failure");
    } finally { scope.close(new Error("测试清理")); }
  });
}
