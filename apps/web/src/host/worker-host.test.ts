import assert from "node:assert/strict";
import test from "node:test";
import { parseProtocolSnapshot } from "./protocol/index.ts";
import { assertWorkerE2EStepAllowed, createWorkerHost, parseWorkerFailure, readWorkerPlayerWorkingOrders, readWorkerSpeedMetrics, refreshWorkerBaseline, requestWorkerIndicators, restoreWorkerSlot, stepWorkerOnce, workerPausePreferenceRequest } from "./worker-host.ts";
import { WorkerRequestScope, type WorkerRequestPort } from "./worker-request.ts";

class FakeWorker implements WorkerRequestPort {
  readonly listeners = new Set<(event: MessageEvent) => void>();
  readonly sent: unknown[] = [];
  addEventListener(_type: "message", listener: (event: MessageEvent) => void): void { this.listeners.add(listener); }
  removeEventListener(_type: "message", listener: (event: MessageEvent) => void): void { this.listeners.delete(listener); }
  postMessage(message: unknown): void { this.sent.push(message); }
  emit(value: unknown): void { for (const listener of this.listeners) listener({ data: value } as MessageEvent); }
}

test("Worker save pins candidate generation and rejects an old saved response after baseline replacement", async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class SaveWorker extends EventTarget {
    static current: SaveWorker;
    readonly sent: Record<string, unknown>[] = [];
    constructor() { super(); SaveWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.sent.push(value); }
    terminate() {}
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: SaveWorker });
  try {
    const ready = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = SaveWorker.current;
    const snapshot = { seq: 42, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
    worker.emit({ type: "created", generation: 1, capabilities: { npcDecisionDiagnostics: false } });
    worker.emit({ type: "baseline", generation: 1, snapshot });
    const host = await ready;
    const candidate = { seq: 42, settledDate: "2030-01-05" };
    const pending = host.save(candidate);
    const request = worker.sent.at(-1)!;
    assert.deepEqual(request, { type: "save", requestId: 1, generation: 1, candidate });
    worker.emit({ type: "baseline", generation: 2, snapshot });
    worker.emit({ type: "saved", requestId: request.requestId, generation: 1, slot: { old: true } });
    await assert.rejects(pending, /generation/);
    host.dispose();
  } finally {
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("Given a generation-correlated Worker metrics response, when read, then it validates the shared speed contract", async () => {
  const worker = new FakeWorker();
  const pending = readWorkerSpeedMetrics(new WorkerRequestScope(worker), 1, 2);
  worker.emit({ type: "speedMetrics", requestId: 1, generation: 2, metrics: {
    requested: { mode: "fixed", multiplier: 60 }, actual_multiplier: 60, sample_duration_ms: 1_000, sample_ticks: 60, running: true,
  } });
  assert.equal((await pending).actual_multiplier, 60);
});

test("Given a generation-correlated Worker order response, when read, then it preserves Money cents", async () => {
  const worker = new FakeWorker();
  const pending = readWorkerPlayerWorkingOrders(new WorkerRequestScope(worker), 5, 2);
  worker.emit({ type: "playerWorkingOrders", requestId: 5, generation: 2, orders: [{
    id: 7, code: "600000", side: "Buy", price: 1234, remainingQty: 200, venue: "auction", frozen: "cash",
  }] });
  assert.deepEqual(await pending, [{
    id: 7, code: "600000", side: "Buy", price: 1234, remainingQty: 200, venue: "auction", frozen: "cash",
  }]);
});

test("Given a typed WASM indicator input error, when returned through the Worker, then calculation rejects", async () => {
  const worker = new FakeWorker();
  const pending = requestWorkerIndicators(new WorkerRequestScope(worker), 6, 2, { prices: [10] });
  worker.emit({ type: "operationError", requestId: 6, generation: 2, message: "non-finite price at index 0" });
  await assert.rejects(pending, /non-finite price at index 0/);
});

test("Given an enriched Worker failure, when parsed, then cause and recovery actions survive", () => {
  assert.deepEqual(parseWorkerFailure({
    type: "failure",
    generation: 1,
    code: "STEP_FATAL",
    where: "wasm-worker.step",
    message: "broken",
    cause: { id: 4 },
    context: { tick: 8 },
    recoverable: true,
    recoveryActions: ["restore"],
  }), {
    code: "STEP_FATAL",
    where: "wasm-worker.step",
    message: "broken",
    cause: { id: 4 },
    context: { tick: 8 },
    recoverable: true,
    recoveryActions: ["restore"],
  });
});

test("Given a restore response from the old request generation, when read, then it yields the new authority before load completes", async () => {
  const worker = new FakeWorker();
  const pending = restoreWorkerSlot(new WorkerRequestScope(worker), { valid: "already-parsed" }, 2, 1);
  worker.emit({
    type: "restored",
    requestId: 2,
    generation: 1,
    nextGeneration: 2,
    snapshot: { seq: 4, tick: 4, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} },
  });
  const restored = await pending;
  assert.equal(restored.nextGeneration, 2);
  assert.equal(restored.snapshot.seq, 4);
  assert.equal(restored.snapshot.tick, 4);
  assert.equal(worker.sent.length, 1);
});

test("Given a refresh request, when the worker reads its live session, then it returns that authoritative snapshot", async () => {
  const worker = new FakeWorker();
  const restoredSnapshot = { seq: 8, tick: 8, day: 1, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
  const pending = refreshWorkerBaseline(new WorkerRequestScope(worker), 12, 4);
  worker.emit({ type: "refreshed", requestId: 12, generation: 4, snapshot: restoredSnapshot });
  assert.deepEqual(await pending, parseProtocolSnapshot(restoredSnapshot, "Worker refreshed.snapshot"));
  assert.deepEqual(worker.sent, [{ type: "refreshBaseline", requestId: 12, generation: 4 }]);
});

test("Given local pause preferences, when sent across the Worker boundary, then the request retains both flags and generation", () => {
  assert.deepEqual(workerPausePreferenceRequest(9, 3, {
    pause_after_close: true,
    pause_before_open: false,
  }), {
    type: "setPausePreferences",
    requestId: 9,
    generation: 3,
    preferences: { pause_after_close: true, pause_before_open: false },
  });
});

test("Given a paused Worker, the correlated single-step response exposes its committed tick", async () => {
  const worker = new FakeWorker();
  const pending = stepWorkerOnce(new WorkerRequestScope(worker), 10, 3);
  worker.emit({ type: "stepped", requestId: 10, generation: 3, tick: 8 });
  assert.equal(await pending, 8);
  assert.deepEqual(worker.sent, [{ type: "stepOnce", requestId: 10, generation: 3 }]);
});

test("Given production or a missing injected capability, Worker E2E stepping is rejected", () => {
  assert.throws(() => assertWorkerE2EStepAllowed(false, true), /E2E 构建/);
  assert.throws(() => assertWorkerE2EStepAllowed(true, false), /未注入/);
  assert.doesNotThrow(() => assertWorkerE2EStepAllowed(true, true));
});

test("Worker initialization passes the requested pool size and reports startup failure immediately", async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class StartupWorker extends EventTarget {
    static current: StartupWorker;
    readonly sent: unknown[] = [];
    terminated = false;
    constructor() { super(); StartupWorker.current = this; }
    postMessage(value: unknown): void { this.sent.push(value); }
    terminate(): void { this.terminated = true; }
    emit(value: unknown): void {
      this.dispatchEvent(Object.assign(new Event("message"), { data: value }));
    }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: StartupWorker });
  try {
    const pending = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n, { threadCount: 128 });
    const worker = StartupWorker.current;
    assert.deepEqual(worker.sent, [{ type: "init", threads: 128 }]);
    worker.emit({ type: "failure", code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.init", message: "线程池启动失败" });
    await assert.rejects(pending, /WASM_WORKER_PROTOCOL @ wasm-worker\.init: 线程池启动失败/);
    assert.equal(worker.terminated, true);
  } finally {
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("duplicate delivered baselines preserve pending order queries, while a new generation invalidates them", async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class BaselineWorker extends EventTarget {
    static current: BaselineWorker;
    readonly sent: Record<string, unknown>[] = [];
    constructor() { super(); BaselineWorker.current = this; }
    postMessage(value: Record<string, unknown>): void { this.sent.push(value); }
    terminate(): void {}
    emit(value: unknown): void {
      this.dispatchEvent(Object.assign(new Event("message"), { data: value }));
    }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: BaselineWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  const snapshot = { seq: 4, tick: 4, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
  try {
    const creating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = BaselineWorker.current;
    worker.emit({ type: "baseline", generation: 1, snapshot });
    host = await creating;
    const updates: unknown[] = [];
    host.start((update) => updates.push(update));
    const refresh = host.refreshBaseline();
    worker.emit({ type: "refreshed", requestId: worker.sent.at(-1)!.requestId, generation: 1, snapshot });
    await refresh;

    const orders = host.playerWorkingOrders();
    const requestId = worker.sent.at(-1)!.requestId;
    worker.emit({ type: "baseline", generation: 1, snapshot });
    worker.emit({ type: "playerWorkingOrders", requestId, generation: 1, orders: [] });
    assert.deepEqual(await orders, []);
    assert.equal(updates.length, 2, "duplicate baseline must not be delivered again");

    const stale = host.playerWorkingOrders();
    const rejected = assert.rejects(stale, /已过期会话 generation/);
    const staleRequestId = worker.sent.at(-1)!.requestId;
    worker.emit({ type: "baseline", generation: 2, snapshot });
    worker.emit({ type: "playerWorkingOrders", requestId: staleRequestId, generation: 1, orders: [] });
    await rejected;
  } finally {
    host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("Worker dispose 与 fatal 保留在途请求原 timeout，后续请求 ID 不与旧请求复用", async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class PendingWorker extends EventTarget {
    static current: PendingWorker;
    readonly sent: Record<string, unknown>[] = [];
    terminations = 0;
    constructor() { super(); PendingWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.sent.push(value); }
    terminate() { this.terminations += 1; }
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: PendingWorker });
  try {
    for (const reason of ["dispose", "fatal"]) {
      const creating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
      const worker = PendingWorker.current;
      const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
      worker.emit({ type: "baseline", generation: 1, snapshot });
      const host = await creating;
      const date = host.civilDate!();
      const preferences = host.setPausePreferences({ pause_after_close: true, pause_before_open: false });
      assert.deepEqual(worker.sent.slice(-2).map((request) => request.requestId), [1, 2]);
      worker.emit({ type: "pausePreferencesSet", requestId: 2, generation: 1 });
      await preferences;
      let settled = false;
      const dateAssertion = assert.rejects(date, /Worker civilDate 操作超时（10000ms）/);
      void date.then(() => { settled = true; }, () => { settled = true; });
      if (reason === "dispose") host.dispose();
      else worker.emit({ type: "failure", code: "TEST_FATAL", where: "worker-test", message: "失败" });
      await Promise.resolve();
      assert.equal(settled, false);
      assert.equal(worker.terminations, 1);
      context.mock.timers.tick(10_000);
      await dateAssertion;
      host.dispose();
    }
  } finally {
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});
