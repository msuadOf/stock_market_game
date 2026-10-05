import assert from "node:assert/strict";
import test from "node:test";
import { parseProtocolSnapshot } from "./protocol/index.ts";
import { assertWorkerE2EStepAllowed, createWorkerHost, parseWorkerFailure, readWorkerPlayerWorkingOrders, readWorkerSpeedMetrics, refreshWorkerBaseline, requestWorkerIndicators, restoreWorkerSlot, stepWorkerOnce, workerPausePreferenceRequest } from "./worker-host.ts";
import { WorkerRequestScope, type WorkerRequestPort } from "./worker-request.ts";
import { ProtocolCoordinator } from "./protocol-coordinator.ts";
import { frame, snapshot as protocolSnapshot, tickBatch } from "./protocol-test-fixtures.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";

class FakeWorker implements WorkerRequestPort {
  readonly listeners = new Set<(event: MessageEvent) => void>();
  readonly sent: unknown[] = [];
  addEventListener(_type: "message", listener: (event: MessageEvent) => void): void { this.listeners.add(listener); }
  removeEventListener(_type: "message", listener: (event: MessageEvent) => void): void { this.listeners.delete(listener); }
  postMessage(message: unknown): void { this.sent.push(message); }
  emit(value: unknown): void { for (const listener of this.listeners) listener({ data: value } as MessageEvent); }
}

for (const operation of ["baseline", "refreshBaseline", "load"] as const) {
  test(`Worker ${operation} baseline 被消费者拒绝时终止并显式失败`, { timeout: 10000 }, async () => {
    const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
    class BaselineRejectingWorker extends EventTarget {
      static current: BaselineRejectingWorker;
      readonly sent: Record<string, unknown>[] = [];
      terminations = 0;
      constructor() { super(); BaselineRejectingWorker.current = this; }
      postMessage(value: Record<string, unknown>) { this.sent.push(value); }
      terminate() { this.terminations++; }
      emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
    }
    Object.defineProperty(globalThis, "Worker", { configurable: true, value: BaselineRejectingWorker });
    let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
    try {
      const creating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
      const worker = BaselineRejectingWorker.current;
      worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot: protocolSnapshot(0, 0) });
      host = await creating;
      let accept = true;
      const failures: unknown[] = [];
      const starting = host.start(() => accept, (failure) => { failures.push(failure); });
      worker.emit({ type: "started", generation: 1, requestId: worker.sent.at(-1)!.requestId });
      await starting;
      accept = false;
      const pendingQuery = host.civilDate!().then(() => null, (error: unknown) => error);
      if (operation === "baseline") {
        worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 2, snapshot: protocolSnapshot(1, 1) });
      } else {
        const pending = operation === "load" ? host.load(currentSaveFixture()) : host.refreshBaseline();
        const rejected = assert.rejects(pending, /消费者拒绝.*baseline/);
        const requestId = worker.sent.at(-1)!.requestId;
        worker.emit(operation === "load"
          ? { type: "restored", civilDate: "2030-01-01", generation: 1, nextGeneration: 2, requestId, snapshot: protocolSnapshot(1, 1) }
          : { type: "refreshed", civilDate: "2030-01-01", generation: 1, requestId, snapshot: protocolSnapshot(1, 1) });
        await rejected;
      }
      assert.equal(worker.terminations, 1);
      assert.match(String(await pendingQuery), /消费者拒绝/);
      assert.equal(failures.length, 1);
      assert.equal(worker.sent.filter((item) => item.type === "uiFrame").length, 0);
      await assert.rejects(host.start(() => true), /已被销毁/);
    } finally {
      await host?.dispose();
      if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
      else Object.defineProperty(globalThis, "Worker", original);
    }
  });
}

test("审计G53：Worker恢复必须推进generation，不能接受相同或更小代", async () => {
  for (const nextGeneration of [1, 2, 3]) {
    const worker = new FakeWorker();
    const requests = new WorkerRequestScope(worker);
    const pending = restoreWorkerSlot(requests, {}, 1, 2);
    worker.emit({ type: "restored", civilDate: "2030-01-01", requestId: 1, generation: 2, nextGeneration,
      snapshot: { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} } });
    if (nextGeneration <= 2) await assert.rejects(pending, /generation.*2.*[12]|[12].*generation.*2/);
    else assert.equal((await pending).nextGeneration, 3);
    assert.equal(requests.pendingCount(), 0);
  }
});

test("审计G53：恢复代次拒绝零、负数、非安全整数和字符串", async () => {
  for (const nextGeneration of [0, -1, 1.5, Number.MAX_SAFE_INTEGER + 1, "3"]) {
    const worker = new FakeWorker();
    const requests = new WorkerRequestScope(worker);
    const pending = restoreWorkerSlot(requests, {}, 1, 2);
    worker.emit({ type: "restored", civilDate: "2030-01-01", requestId: 1, generation: 2, nextGeneration,
      snapshot: { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} } });
    await assert.rejects(pending, /正安全整数/);
    assert.equal(requests.pendingCount(), 0);
  }
});

test("Worker refresh 已回包但 continuation 前 dispose 不得复活 baseline", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class DisposedRefreshWorker extends EventTarget {
    static current: DisposedRefreshWorker;
    readonly sent: Record<string, unknown>[] = [];
    constructor() { super(); DisposedRefreshWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.sent.push(value); }
    terminate() {}
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: DisposedRefreshWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = DisposedRefreshWorker.current;
    worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot: protocolSnapshot(0, 0) });
    host = await creating;
    const pending = host.refreshBaseline();
    const rejected = assert.rejects(pending, /已过期会话 generation/);
    worker.emit({ type: "refreshed", civilDate: "2030-01-01", generation: 1, requestId: worker.sent.at(-1)!.requestId, snapshot: protocolSnapshot(1, 1) });
    await host.dispose();
    await rejected;
    assert.throws(() => host!.snapshot(), /快照尚未就绪/);
  } finally {
    await host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

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
    worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
    const host = await ready;
    const candidate = { seq: 42, settledDate: "2030-01-05" };
    const pending = host.save(candidate);
    const request = worker.sent.at(-1)!;
    assert.deepEqual(request, { type: "save", requestId: 1, generation: 1, candidate });
    worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 2, snapshot });
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
    id: 7, code: "600000", side: "Buy", price: "1234", remainingQty: 200, venue: "auction", frozen: "cash",
  }] });
  assert.deepEqual(await pending, [{
    id: 7, code: "600000", side: "Buy", price: "1234", remainingQty: 200, venue: "auction", frozen: "cash",
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
    type: "restored", civilDate: "2030-01-01",
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
  worker.emit({ type: "refreshed", civilDate: "2030-01-01", requestId: 12, generation: 4, snapshot: restoredSnapshot });
  assert.deepEqual(await pending, { snapshot: parseProtocolSnapshot(restoredSnapshot, "Worker refreshed.snapshot"), civilDate: "2030-01-01" });
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
    worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
    host = await creating;
    const updates: unknown[] = [];
    const starting = host.start((update) => { updates.push(update); });
    worker.emit({ type: "started", requestId: worker.sent.at(-1)!.requestId, generation: 1 });
    await starting;
    const refresh = host.refreshBaseline();
    worker.emit({ type: "refreshed", civilDate: "2030-01-01", requestId: worker.sent.at(-1)!.requestId, generation: 1, snapshot });
    await refresh;

    const orders = host.playerWorkingOrders();
    const requestId = worker.sent.at(-1)!.requestId;
    worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
    worker.emit({ type: "playerWorkingOrders", requestId, generation: 1, orders: [] });
    assert.deepEqual(await orders, []);
    assert.equal(updates.length, 2, "duplicate baseline must not be delivered again");

    const stale = host.playerWorkingOrders();
    const rejected = assert.rejects(stale, /已过期会话 generation/);
    const staleRequestId = worker.sent.at(-1)!.requestId;
    worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 2, snapshot });
    worker.emit({ type: "playerWorkingOrders", requestId: staleRequestId, generation: 1, orders: [] });
    await rejected;
  } finally {
    host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("Worker 控制等待应用回执，protocolBatch 每个提交接纳后立即 ACK 且不等待 render", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class ControlledWorker extends EventTarget {
    static current: ControlledWorker;
    readonly sent: Record<string, unknown>[] = [];
    terminations = 0;
    constructor() { super(); ControlledWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.sent.push(value); }
    terminate() { this.terminations++; }
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: ControlledWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = ControlledWorker.current;
    const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
    worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
    host = await creating;
    const updates: unknown[] = [];
    let applied = false;
    let failures = 0;
    const starting = host.start((update) => { updates.push(update); }, () => { failures++; }).then(() => { applied = true; });
    await Promise.resolve();
    assert.equal(applied, false);
    worker.emit({ type: "started", generation: 1, requestId: worker.sent.at(-1)!.requestId });
    await starting;
    const commits = [{ TickBatch: { frames: ["完整事实一"] } }, { CivilUpdate: { facts: ["完整事实二"] } }];
    worker.emit({ type: "protocolBatch", generation: 1, deliveryId: 3, updates: commits });
    assert.deepEqual(updates.slice(1).map((update) => (update as { update: unknown }).update), commits);
    assert.deepEqual(worker.sent.at(-1), { type: "uiFrame", generation: 1, deliveryId: 3 });
    const speedChange = assert.rejects(host.setSpeed(10), /已过期 generation/);
    const requestId = worker.sent.at(-1)!.requestId;
    worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 2, snapshot });
    worker.emit({ type: "speedSet", generation: 1, requestId });
    await speedChange;
    const acceptedCount = updates.length;
    const acknowledgements = worker.sent.filter((message) => message.type === "uiFrame").length;
    worker.emit({ type: "failure", code: "TEST_FATAL", where: "worker-test", message: "失败" });
    worker.emit({ type: "protocol", generation: 2, deliveryId: 4, update: commits[0] });
    worker.emit({ type: "failure", code: "REPEATED_FATAL", where: "worker-test", message: "重复失败" });
    assert.equal(updates.length, acceptedCount);
    assert.equal(worker.sent.filter((message) => message.type === "uiFrame").length, acknowledgements);
    assert.equal(worker.terminations, 1);
    assert.equal(failures, 1);
    await assert.rejects(host.start(() => {}), /已被销毁/);
  } finally {
    await host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("Worker cached baseline 拒绝不启动或 ACK，显式重试仍交付且旧 host 不干扰新 host", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class IsolatedWorker extends EventTarget {
    static current: IsolatedWorker;
    readonly sent: Record<string, unknown>[] = [];
    constructor() { super(); IsolatedWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.sent.push(value); }
    terminate() {}
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: IsolatedWorker });
  let oldHost: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  let newHost: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const oldCreating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const oldWorker = IsolatedWorker.current;
    oldWorker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot: protocolSnapshot(0, 0) });
    oldHost = await oldCreating;
    const rejectingStart = oldHost.start(() => false);
    const unexpectedStart = oldWorker.sent.find((message) => message.type === "start");
    if (unexpectedStart !== undefined) oldWorker.emit({ type: "started", generation: 1, requestId: unexpectedStart.requestId });
    await assert.rejects(rejectingStart, /消费者拒绝.*baseline/);
    assert.equal(oldWorker.sent.filter((message) => message.type === "start" || message.type === "uiFrame").length, 0);
    const newCreating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 2n);
    const newWorker = IsolatedWorker.current;
    newWorker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot: protocolSnapshot(9, 4) });
    newHost = await newCreating;
    const newUpdates: unknown[] = [];
    let newStarted = false;
    const startingNew = newHost.start((update) => { newUpdates.push(update); }).then(() => { newStarted = true; });
    const requestId = newWorker.sent.at(-1)!.requestId;
    oldWorker.emit({ type: "started", generation: 1, requestId });
    oldWorker.emit({ type: "protocol", generation: 1, deliveryId: 1, update: {} });
    await Promise.resolve();
    assert.equal(newStarted, false);
    assert.equal(newUpdates.length, 1);
    assert.equal(oldWorker.sent.filter((message) => message.type === "uiFrame").length, 0);
    newWorker.emit({ type: "started", generation: 1, requestId });
    await startingNew;
    const retriedUpdates: unknown[] = [];
    const retry = oldHost.start((update) => { retriedUpdates.push(update); return true; });
    oldWorker.emit({ type: "started", generation: 1, requestId: oldWorker.sent.at(-1)!.requestId });
    await retry;
    assert.equal(retriedUpdates.length, 1);
    assert.equal(oldWorker.sent.filter((message) => message.type === "start").length, 1);
    assert.equal(newUpdates.length, 1);
    assert.equal(newHost.tick(), 9);
  } finally {
    await oldHost?.dispose();
    await newHost?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("Worker dispose 与 fatal 立即取消在途请求，释放监听并拒绝后续请求", async (context) => {
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
      worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
      const host = await creating;
      const date = host.civilDate!();
      const preferences = host.setPausePreferences({ pause_after_close: true, pause_before_open: false });
      assert.deepEqual(worker.sent.slice(-2).map((request) => request.requestId), [1, 2]);
      worker.emit({ type: "pausePreferencesSet", requestId: 2, generation: 1 });
      await preferences;
      let settled = false;
      const dateAssertion = assert.rejects(date, /已被销毁|TEST_FATAL/);
      void date.then(() => { settled = true; }, () => { settled = true; });
      if (reason === "dispose") host.dispose();
      else worker.emit({ type: "failure", code: "TEST_FATAL", where: "worker-test", message: "失败" });
      await dateAssertion;
      assert.equal(settled, true);
      assert.equal(worker.terminations, 1);
      await assert.rejects(host.readSpeedMetrics(), /已被销毁|TEST_FATAL/);
      host.dispose();
    }
  } finally {
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("Worker 真实 coordinator 拒绝批次中间提交后不消费尾部、不 ACK、立即终止并取消控制", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class RejectingWorker extends EventTarget {
    static current: RejectingWorker;
    readonly sent: Record<string, unknown>[] = [];
    terminations = 0;
    constructor() { super(); RejectingWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.sent.push(value); }
    terminate() { this.terminations++; }
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: RejectingWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createWorkerHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = RejectingWorker.current;
    worker.emit({ type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot: protocolSnapshot(0, 0) });
    host = await creating;
    let applications = 0;
    const failures: unknown[] = [];
    const coordinator = new ProtocolCoordinator({ onBaseline: () => {}, onApplied: () => { applications++; }, onFailure: (failure) => { failures.push(failure); } });
    const starting = host.start((update) => coordinator.accept(update));
    worker.emit({ type: "started", generation: 1, requestId: worker.sent.at(-1)!.requestId });
    await starting;
    const pending = assert.rejects(host.setSpeed(10), /消费者拒绝/);
    worker.emit({ type: "protocolBatch", generation: 1, deliveryId: 7, updates: [
      tickBatch([frame(1, 0, ["600000"])], protocolSnapshot(1, 1)),
      tickBatch([frame(3, 1, ["600000"])], protocolSnapshot(3, 2)),
      tickBatch([frame(2, 1, ["600000"])], protocolSnapshot(2, 2)),
    ] });
    await pending;
    assert.equal(applications, 1);
    assert.equal(failures.length, 1);
    assert.equal(coordinator.status().kind, "failure");
    assert.equal(worker.sent.filter((message) => message.type === "uiFrame").length, 0);
    assert.equal(worker.terminations, 1);
    worker.emit({ type: "protocol", generation: 1, deliveryId: 8, update: tickBatch([frame(1, 0, ["600000"])], protocolSnapshot(1, 1)) });
    assert.equal(applications, 1);
    await host.dispose();
    assert.equal(worker.terminations, 1);
  } finally {
    await host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});
