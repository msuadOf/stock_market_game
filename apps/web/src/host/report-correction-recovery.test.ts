import assert from "node:assert/strict";
import test from "node:test";
import { createWorkerHost } from "./worker-host.ts";
import { snapshot } from "./protocol-test-fixtures.ts";

class FailureWorker extends EventTarget {
  static readonly instances: FailureWorker[] = [];
  readonly sent: Record<string, unknown>[] = [];
  terminations = 0;
  constructor() { super(); FailureWorker.instances.push(this); }
  postMessage(value: Record<string, unknown>) { this.sent.push(value); }
  terminate() { this.terminations++; }
  emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
}

test("Worker created前真实failure立即暴露原始错误，不吞成初始化超时", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: FailureWorker });
  try {
    const creating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = FailureWorker.instances.at(-1)!;
    const rejection = assert.rejects(creating, /真实 capabilities 错误/);
    worker.emit({ type: "failure", generation: 1, code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.create", message: "真实 capabilities 错误" });
    assert.equal(worker.terminations, 1);
    await rejection;
  } finally {
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

for (const generation of [undefined, "1", -1]) {
  test(`Worker failure非法generation ${String(generation)} 显式拒绝而非吞错`, { timeout: 10000 }, async () => {
    const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
    Object.defineProperty(globalThis, "Worker", { configurable: true, value: FailureWorker });
    try {
      const creating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
      const worker = FailureWorker.instances.at(-1)!;
      const rejection = assert.rejects(creating, /Worker failure generation 必须是非负安全整数/);
      worker.emit({ type: "failure", generation, code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.create", message: "坏scope" });
      assert.equal(worker.terminations, 1);
      await rejection;
    } finally {
      if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
      else Object.defineProperty(globalThis, "Worker", original);
    }
  });
}

for (const failure of [
  { code: "STEP_FATAL", message: "ReportCorrection rollback 失败", recoverable: true },
  { code: "REPORT_CORRECTION_REJECTED", message: "不可恢复", recoverable: false },
]) {
  test(`Worker ${failure.code}/${failure.recoverable} 不按文字误判可恢复`, { timeout: 10000 }, async () => {
    const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
    Object.defineProperty(globalThis, "Worker", { configurable: true, value: FailureWorker });
    let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
    try {
      const creating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
      const worker = FailureWorker.instances.at(-1)!;
      worker.emit({ type: "baseline", generation: 1, snapshot: snapshot(0, 0) });
      host = await creating;
      worker.emit({ type: "failure", generation: 1, where: "session.dayEnd", ...failure });
      assert.equal(worker.terminations, 1);
      await assert.rejects(host.queryReportCorrections(), /已过期|已关闭|STEP_FATAL|REPORT_CORRECTION/);
    } finally {
      await host?.dispose();
      if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
      else Object.defineProperty(globalThis, "Worker", original);
    }
  });
}

test("Worker财报更正日终拒绝保留健康查询、取消和恢复能力，旧generation failure隔离", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class FakeWorker extends EventTarget {
    static readonly instances: FakeWorker[] = [];
    readonly sent: Record<string, unknown>[] = [];
    terminations = 0;
    constructor() { super(); FakeWorker.instances.push(this); }
    postMessage(value: Record<string, unknown>) { this.sent.push(value); }
    terminate() { this.terminations++; }
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: FakeWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = FakeWorker.instances[0]!;
    worker.emit({ type: "baseline", generation: 1, snapshot: snapshot(0, 0) });
    host = await creating;
    const failures: unknown[] = [];
    const starting = host.start(() => true, (failure) => failures.push(failure));
    worker.emit({ type: "started", requestId: worker.sent.at(-1)!.requestId, generation: 1 });
    await starting;
    const failure = { type: "failure", generation: 1, code: "REPORT_CORRECTION_REJECTED", where: "session.endCivilDay", message: "真实日终更正失败，无部分账务", recoverable: true, recoveryActions: ["取消待处理后重试"] };
    worker.emit({ ...failure, generation: 0 });
    assert.equal(failures.length, 0);
    worker.emit(failure);
    assert.equal(worker.terminations, 0);
    assert.equal(failures.length, 1);
    const query = host.queryReportCorrections();
    worker.emit({ type: "reportCorrections", requestId: worker.sent.at(-1)!.requestId, generation: 1, value: { pending: [], completed: {} } });
    assert.deepEqual(await query, { pending: [], completed: {} });
    const cancel = host.cancelReportCorrection("correct-1");
    worker.emit({ type: "reportCorrectionCancelled", requestId: worker.sent.at(-1)!.requestId, generation: 1 });
    await cancel;
  } finally {
    await host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});
