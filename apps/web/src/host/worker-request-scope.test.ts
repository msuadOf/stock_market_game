import assert from "node:assert/strict";
import test from "node:test";
import * as requests from "./worker-request.ts";
import type { WorkerRequestPort } from "./worker-request.ts";

class Port implements WorkerRequestPort {
  readonly listeners = new Set<(event: MessageEvent) => void>();
  readonly sent: unknown[] = [];
  addEventListener(_type: "message", listener: (event: MessageEvent) => void) { this.listeners.add(listener); }
  removeEventListener(_type: "message", listener: (event: MessageEvent) => void) { this.listeners.delete(listener); }
  postMessage(message: unknown) { this.sent.push(message); }
  emit(data: unknown) { for (const listener of this.listeners) listener({ data } as MessageEvent); }
}

test("WorkerRequestScope 为同一 port 分配连续 ID，逆序完成只清理对应资源", async () => {
  assert.equal(typeof requests.WorkerRequestScope, "function", "请求资源应由 WorkerRequestScope 统一拥有");
  const port = new Port();
  const scope = new requests.WorkerRequestScope(port);
  const firstId = scope.nextRequestId();
  const first = scope.request({ type: "save", requestId: firstId, generation: 1 }, "saved");
  const secondId = scope.nextRequestId();
  const second = scope.request({ type: "civilDate", requestId: secondId, generation: 2 }, "civilDate");
  assert.deepEqual([firstId, secondId], [1, 2]);
  assert.equal(scope.pendingCount(), 2);
  port.emit({ type: "saved", requestId: firstId, generation: 2 });
  assert.equal(scope.pendingCount(), 2);
  port.emit({ type: "civilDate", requestId: secondId, generation: 2, date: "2030-01-02" });
  assert.equal((await second).date, "2030-01-02");
  assert.equal(scope.pendingCount(), 1);
  assert.equal(port.listeners.size, 1);
  port.emit({ type: "saved", requestId: firstId, generation: 1, slot: "current" });
  assert.equal((await first).slot, "current");
  port.emit({ type: "saved", requestId: firstId, generation: 1, slot: "duplicate" });
  assert.equal(scope.pendingCount(), 0);
  assert.equal(port.listeners.size, 0);
  assert.equal(scope.nextRequestId(), 3);
});

test("WorkerRequestScope 的 operationError 与 timeout 回收 listener 和登记", async (context) => {
  assert.equal(typeof requests.WorkerRequestScope, "function", "请求资源应由 WorkerRequestScope 统一拥有");
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const port = new Port();
  const scope = new requests.WorkerRequestScope(port);
  const failed = scope.request({ type: "restore", requestId: scope.nextRequestId(), generation: 1 }, "restored", 20);
  const failedAssertion = assert.rejects(failed, /存档校验失败/);
  port.emit({ type: "operationError", requestId: 1, generation: 1, message: "存档校验失败" });
  await failedAssertion;
  const timedOut = scope.request({ type: "save", requestId: scope.nextRequestId(), generation: 1 }, "saved", 20);
  const timeoutAssertion = assert.rejects(timedOut, /Worker save 操作超时（20ms）/);
  context.mock.timers.tick(20);
  await timeoutAssertion;
  assert.equal(scope.pendingCount(), 0);
  assert.equal(port.listeners.size, 0);
});

test("WorkerRequestScope postMessage 同步抛错立即释放资源，晚到 timeout 不影响后续请求", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const sendFailure = new Error("postMessage 失败");
  class ThrowingPort extends Port {
    failing = true;
    postMessage(message: unknown) {
      if (this.failing) throw sendFailure;
      super.postMessage(message);
    }
  }
  const port = new ThrowingPort();
  const scope = new requests.WorkerRequestScope(port);
  await assert.rejects(scope.request({ type: "save", requestId: scope.nextRequestId(), generation: 1 }, "saved", 20), (error) => error === sendFailure);
  assert.equal(scope.pendingCount(), 0);
  assert.equal(port.listeners.size, 0);
  port.failing = false;
  const retryId = scope.nextRequestId();
  const retry = scope.request({ type: "save", requestId: retryId, generation: 1 }, "saved", 40);
  context.mock.timers.tick(20);
  assert.equal(scope.pendingCount(), 1);
  assert.equal(port.listeners.size, 1);
  port.emit({ type: "saved", requestId: retryId, generation: 1, slot: "retried" });
  assert.equal((await retry).slot, "retried");
  assert.equal(scope.pendingCount(), 0);
  assert.equal(port.listeners.size, 0);
});
