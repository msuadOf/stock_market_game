import { defaultCatalogFixture, catalogWorkerFixture } from "./catalog-test-fixture.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { bootstrapLegacy, createLegacyHost, drainLegacyPorts, LegacyWorker } from "./worker-host-legacy.test-support.ts";
import { parseProtocolSnapshot } from "./protocol/index.ts";
import { assertWorkerE2EStepAllowed, createWorkerHost, parseWorkerFailure, readWorkerPlayerWorkingOrders, readWorkerStockHistory, readWorkerSpeedMetrics, refreshWorkerBaseline, requestWorkerIndicators, restoreWorkerSlot, stepWorkerOnce, workerPausePreferenceRequest } from "./worker-host.ts";
import { WorkerRequestScope, type WorkerRequestPort } from "./worker-request.ts";
import { ProtocolCoordinator } from "./protocol-coordinator.ts";
import { frame, snapshot as protocolSnapshot, tickBatch, market, dailyCandle, mirrorOrigin } from "./protocol-test-fixtures.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";
import { drain as drainW1, fixture as fixtureW1 } from "./worker-host-w1.test-support.ts";


class FakeWorker implements WorkerRequestPort {
  readonly listeners = new Set<(event: MessageEvent) => void>();
  readonly sent: unknown[] = [];
  addEventListener(_type: "message", listener: (event: MessageEvent) => void): void { this.listeners.add(listener); }
  removeEventListener(_type: "message", listener: (event: MessageEvent) => void): void { this.listeners.delete(listener); }
  postMessage(message: unknown): void { this.sent.push(message); }
  emit(value: unknown): void { for (const listener of this.listeners) listener({ data: value } as MessageEvent); }
}

test("非共享 substrate 显式失败，不生成 intake、不通过 engine enqueue 静默降级", { timeout: 10000 }, async () => {
  const f = fixtureW1();
  try {
    f.launch(); await drainW1();
    f.ingress(1, 1, { shared: false }); f.baseline(); await drainW1();
    assert.equal(f.outcome.status, "rejected", "不支持共享线程时必须显式失败，不能确认未执行的命令");
    assert.equal(f.workers.length, 1, "非共享 substrate 不得 spawn intake worker");
    assert.equal(f.workers[0].sent.some((request) => request.type === "enqueue"), false);
    assert.equal(f.workers[0].terminations, 1);
  } finally { await f.cleanup(); }
});

test("Browser engine worker 忙碌时 submitIntent 由独立 intake worker 确认，dispose 同时关闭两者", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class IntakeWorker extends LegacyWorker {
    static readonly instances: IntakeWorker[] = [];
    readonly sent: Record<string, unknown>[] = [];
    terminations = 0;
    constructor() { super(); IntakeWorker.instances.push(this); }
    postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
    terminate() { this.terminations++; }
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: IntakeWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const engine = IntakeWorker.instances[0];
    await bootstrapLegacy(engine, false);
    engine.emit({ type: "ingress", generation: 1, token: 42, shared: true });
    await drainLegacyPorts();
    const intake = IntakeWorker.instances[1];
    const binding = intake.sent[0];
    assert.equal(binding.type, "bindIngress");
    intake.emit({ type: "ingressBound", requestId: binding.requestId, generation: 1 });
    await drainLegacyPorts();
    // ADR 0041: baseline wire 契约携带 civilDate，E8 改写时遗漏导致协议解析失败
    engine.emit({ ...catalogWorkerFixture(protocolSnapshot(0, 0)), type: "baseline", civilDate: "2030-01-02", generation: 1, snapshot: protocolSnapshot(0, 0) });
    host = await creating;
    const intent = { Cancel: { code: "000812", id: 7 } } as Parameters<typeof host.submitIntent>[0];
    const submitting = host.submitIntent(intent);
    await drainLegacyPorts();
    const request = intake.sent.at(-1)!;
    assert.equal(request.type, "enqueue");
    // 公共 Intent（代码串）入队前已译为 wire 数值 StockId（ADR 0041）。
    assert.deepEqual(request.intent, { Cancel: { code: 0, id: 7 } });
    assert.equal(engine.sent.some((message) => message.type === "enqueue"), false);
    intake.emit({ type: "enqueued", requestId: request.requestId, generation: 1 });
    await submitting;
    await host.dispose();
    assert.equal(engine.terminations, 1);
    assert.equal(intake.terminations, 1);
    await assert.rejects(host.submitIntent(intent), /已过期|已关闭/);
  } finally {
    await host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

for (const scenario of ["consumerReject", "initializationTimeout", "staleEnqueued", "disposedEnqueued"] as const) {
  test(`Browser ingress ${scenario} 不泄漏 intake worker 或误报旧代成功`, { timeout: 10000 }, async (context) => {
    context.mock.timers.enable({ apis: ["setTimeout"] });
    const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
    class IntakeWorker extends LegacyWorker {
      static readonly instances: IntakeWorker[] = [];
      readonly sent: Record<string, unknown>[] = [];
      terminations = 0;
      constructor() { super(); IntakeWorker.instances.push(this); }
      postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
      terminate() { this.terminations++; }
      emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
    }
    Object.defineProperty(globalThis, "Worker", { configurable: true, value: IntakeWorker });
    let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
    try {
      const creating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
      const engine = IntakeWorker.instances[0];
      await bootstrapLegacy(engine, false);
      if (scenario === "initializationTimeout") context.mock.timers.tick(1);
      engine.emit({ type: "ingress", generation: 1, token: 42, shared: true });
      const intake = IntakeWorker.instances[1];
      if (scenario === "initializationTimeout") {
        const rejected = assert.rejects(creating, /初始化超时/);
        context.mock.timers.tick(9999);
        await rejected;
        assert.equal(engine.terminations, 1);
        assert.equal(intake.terminations, 1);
        return;
      }
      intake.emit({ type: "ingressBound", requestId: intake.sent[0].requestId, generation: 1 });
      // ADR 0041: baseline wire 契约携带 civilDate，E8 改写时遗漏导致协议解析失败
      engine.emit({ ...catalogWorkerFixture(protocolSnapshot(0, 0)), type: "baseline", civilDate: "2030-01-02", generation: 1, snapshot: protocolSnapshot(0, 0) });
      host = await creating;
      const intent = { Cancel: { code: "000812", id: 7 } } as Parameters<typeof host.submitIntent>[0];
      const pending = host.submitIntent(intent);
      await Promise.resolve();
      const rejected = assert.rejects(pending, /消费者拒绝|已过期/);
      if (scenario === "consumerReject") {
        let accept = true;
        const starting = host.start(() => accept);
        engine.emit({ type: "started", generation: 1, requestId: engine.sent.at(-1)!.requestId });
        await starting;
        accept = false;
        engine.emit({ type: "protocol", generation: 1, deliveryId: 1, update: tickBatch([frame(1, 0, [0])], protocolSnapshot(1, 1)) });
      } else {
        const request = intake.sent.at(-1)!;
        intake.emit({ type: "enqueued", generation: 1, requestId: request.requestId });
        if (scenario === "disposedEnqueued") await host.dispose();
        else engine.emit({ ...catalogWorkerFixture(protocolSnapshot(1, 1)), type: "baseline", civilDate: "2030-01-02", generation: 2, snapshot: protocolSnapshot(1, 1) });
      }
      if (scenario === "consumerReject") {
        assert.equal(intake.terminations, 1);
      }
      await rejected;
      await host.dispose();
      assert.equal(engine.terminations, 1);
      assert.equal(intake.terminations, 1);
    } finally {
      await host?.dispose();
      if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
      else Object.defineProperty(globalThis, "Worker", original);
    }
  });
}

test("Worker 股票历史请求绑定 requestId 与 generation，且不携带可指定账户", { timeout: 10000 }, async () => {
  const worker = new FakeWorker();
  const requests = new WorkerRequestScope(worker);
  const pending = readWorkerStockHistory(requests, 12, 3, "000812", defaultCatalogFixture);
  assert.deepEqual(worker.sent, [{ type: "stockHistory", requestId: 12, generation: 3, code: 0 }]);
  worker.emit({ type: "stockHistory", requestId: 12, generation: 3, data: { code: 0, daily_candles: [], active_daily_candle: null } });
  assert.deepEqual(await pending, { code: "000812", daily_candles: [], active_daily_candle: null });
  assert.equal(requests.pendingCount(), 0);
});

for (const operation of ["baseline", "refreshBaseline", "load"] as const) {
  test(`Worker ${operation} baseline 被消费者拒绝时终止并显式失败`, { timeout: 10000 }, async () => {
    const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
    class BaselineRejectingWorker extends LegacyWorker {
      static current: BaselineRejectingWorker;
      readonly sent: Record<string, unknown>[] = [];
      terminations = 0;
      constructor() { super(); BaselineRejectingWorker.current = this; }
      postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
      terminate() { this.terminations++; }
      emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
    }
    Object.defineProperty(globalThis, "Worker", { configurable: true, value: BaselineRejectingWorker });
    let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
    try {
      const creating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
      const worker = BaselineRejectingWorker.current;
      await bootstrapLegacy(worker);
      worker.emit({ ...catalogWorkerFixture(protocolSnapshot(0, 0)), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot: protocolSnapshot(0, 0) });
      host = await creating;
      let accept = true;
      const failures: unknown[] = [];
      const starting = host.start(() => accept, (failure) => { failures.push(failure); });
      worker.emit({ type: "started", generation: 1, requestId: worker.sent.at(-1)!.requestId });
      await starting;
      accept = false;
      const pendingQuery = host.civilDate!().then(() => null, (error: unknown) => error);
      if (operation === "baseline") {
        worker.emit({ ...catalogWorkerFixture(protocolSnapshot(1, 1)), type: "baseline", civilDate: "2030-01-01", generation: 2, snapshot: protocolSnapshot(1, 1) });
      } else {
        const pending = operation === "load" ? host.load(currentSaveFixture()) : host.refreshBaseline();
        const rejected = assert.rejects(pending, /消费者拒绝.*baseline/);
        const requestId = worker.sent.at(-1)!.requestId;
        worker.emit(operation === "load"
          ? { ...catalogWorkerFixture(protocolSnapshot(1, 1)), type: "restored", civilDate: "2030-01-01", generation: 1, nextGeneration: 2, requestId, snapshot: protocolSnapshot(1, 1) }
          : { ...catalogWorkerFixture(protocolSnapshot(1, 1)), type: "refreshed", civilDate: "2030-01-01", generation: 1, requestId, snapshot: protocolSnapshot(1, 1) });
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
    worker.emit({ ...catalogWorkerFixture({ seq: 0, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} }), type: "restored", civilDate: "2030-01-01", requestId: 1, generation: 2, nextGeneration,
      snapshot: { seq: 0, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} } });
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
    worker.emit({ ...catalogWorkerFixture({ seq: 0, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} }), type: "restored", civilDate: "2030-01-01", requestId: 1, generation: 2, nextGeneration,
      snapshot: { seq: 0, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} } });
    await assert.rejects(pending, /正安全整数/);
    assert.equal(requests.pendingCount(), 0);
  }
});

test("Worker refresh 已回包但 continuation 前 dispose 不得复活 baseline", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class DisposedRefreshWorker extends LegacyWorker {
    static current: DisposedRefreshWorker;
    readonly sent: Record<string, unknown>[] = [];
    constructor() { super(); DisposedRefreshWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
    terminate() {}
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: DisposedRefreshWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = DisposedRefreshWorker.current;
    await bootstrapLegacy(worker);
    worker.emit({ ...catalogWorkerFixture(protocolSnapshot(0, 0)), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot: protocolSnapshot(0, 0) });
    host = await creating;
    const pending = host.refreshBaseline();
    const rejected = assert.rejects(pending, /已过期会话 generation/);
    worker.emit({ ...catalogWorkerFixture(protocolSnapshot(1, 1)), type: "refreshed", civilDate: "2030-01-01", generation: 1, requestId: worker.sent.at(-1)!.requestId, snapshot: protocolSnapshot(1, 1) });
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
  class SaveWorker extends LegacyWorker {
    static current: SaveWorker;
    readonly sent: Record<string, unknown>[] = [];
    constructor() { super(); SaveWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
    terminate() {}
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: SaveWorker });
  try {
    const ready = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = SaveWorker.current;
    await bootstrapLegacy(worker);
    const snapshot = { seq: 42, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
    worker.emit({ type: "created", generation: 1, capabilities: { npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false }, personalTradeHistory: false } });
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
    const host = await ready;
    const candidate = { seq: 42, settledDate: "2030-01-05" };
    const pending = host.save(candidate);
    const request = worker.sent.at(-1)!;
    assert.deepEqual(request, { type: "save", requestId: 1, generation: 1, candidate });
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-01", generation: 2, snapshot });
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
  const pending = readWorkerPlayerWorkingOrders(new WorkerRequestScope(worker), 5, 2, defaultCatalogFixture);
  worker.emit({ type: "playerWorkingOrders", requestId: 5, generation: 2, orders: [{
    id: 7, code: 0, side: "Buy", price: "1234", remainingQty: 200, venue: "Auction", frozen: "Cash",
  }] });
  assert.deepEqual(await pending, [{
    id: 7, code: "000812", side: "Buy", price: "1234", remainingQty: 200, venue: "Auction", frozen: "Cash",
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
  worker.emit({ ...catalogWorkerFixture({ seq: 4, tick: 4, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(4), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} }),
    type: "restored", civilDate: "2030-01-01",
    requestId: 2,
    generation: 1,
    nextGeneration: 2,
    snapshot: { seq: 4, tick: 4, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(4), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} },
  });
  const restored = await pending;
  assert.equal(restored.nextGeneration, 2);
  assert.equal(restored.snapshot.seq, 4);
  assert.equal(restored.snapshot.tick, 4);
  assert.equal(worker.sent.length, 1);
});

test("Given a refresh request, when the worker reads its live session, then it returns that authoritative snapshot", async () => {
  const worker = new FakeWorker();
  const restoredSnapshot = { seq: 8, tick: 8, day: 1, phase: "Continuous", mirror_origin: mirrorOrigin(8), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
  const pending = refreshWorkerBaseline(new WorkerRequestScope(worker), 12, 4);
  worker.emit({ ...catalogWorkerFixture(restoredSnapshot), type: "refreshed", civilDate: "2030-01-01", requestId: 12, generation: 4, snapshot: restoredSnapshot });
  const baseline = await pending;
  assert.deepEqual(baseline.snapshot, parseProtocolSnapshot(restoredSnapshot, "Worker refreshed.snapshot", defaultCatalogFixture));
  assert.equal(baseline.civilDate, "2030-01-01");
  assert.deepEqual(baseline.stockCatalog, []);
  assert.deepEqual(baseline.companyCatalog, []);
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

test("Worker initialization sends init without pool size and reports startup failure immediately", async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class StartupWorker extends LegacyWorker {
    static current: StartupWorker;
    readonly sent: unknown[] = [];
    terminated = false;
    constructor() { super(); StartupWorker.current = this; }
    postMessage(value: unknown): void { this.prepare(value as Record<string, unknown>); this.sent.push(value); }
    terminate(): void { this.terminated = true; }
    emit(value: unknown): void {
      this.dispatchEvent(Object.assign(new Event("message"), { data: value }));
    }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: StartupWorker });
  try {
    const pending = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = StartupWorker.current;
    const init = worker.sent[0] as Record<string, unknown>;
    assert.equal(init.type, "init");
    assert.deepEqual(init.setup, {});
    assert.ok(init.controlPort instanceof MessagePort);
    assert.equal(Object.hasOwn(init, "poolSize"), false);
    worker.emit({ type: "failure", code: "WASM_WORKER_PROTOCOL", where: "wasm-worker.init", message: "初始化失败" });
    await assert.rejects(pending, /WASM_WORKER_PROTOCOL @ wasm-worker\.init: 初始化失败/);
    assert.equal(worker.terminated, true);
  } finally {
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("duplicate delivered baselines preserve pending order queries, while a new generation invalidates them", async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class BaselineWorker extends LegacyWorker {
    static current: BaselineWorker;
    readonly sent: Record<string, unknown>[] = [];
    constructor() { super(); BaselineWorker.current = this; }
    postMessage(value: Record<string, unknown>): void { this.prepare(value); this.sent.push(value); }
    terminate(): void {}
    emit(value: unknown): void {
      this.dispatchEvent(Object.assign(new Event("message"), { data: value }));
    }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: BaselineWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  const snapshot = { seq: 4, tick: 4, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(4), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
  try {
    const creating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = BaselineWorker.current;
    await bootstrapLegacy(worker);
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
    host = await creating;
    const updates: unknown[] = [];
    const starting = host.start((update) => { updates.push(update); });
    worker.emit({ type: "started", requestId: worker.sent.at(-1)!.requestId, generation: 1 });
    await starting;
    const refresh = host.refreshBaseline();
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "refreshed", civilDate: "2030-01-01", requestId: worker.sent.at(-1)!.requestId, generation: 1, snapshot });
    await refresh;

    const orders = host.playerWorkingOrders();
    const requestId = worker.sent.at(-1)!.requestId;
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
    worker.emit({ type: "playerWorkingOrders", requestId, generation: 1, orders: [] });
    assert.deepEqual(await orders, []);
    assert.equal(updates.length, 2, "duplicate baseline must not be delivered again");

    const stale = host.playerWorkingOrders();
    const rejected = assert.rejects(stale, /已过期会话 generation/);
    const staleRequestId = worker.sent.at(-1)!.requestId;
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-01", generation: 2, snapshot });
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
  class ControlledWorker extends LegacyWorker {
    static current: ControlledWorker;
    readonly sent: Record<string, unknown>[] = [];
    terminations = 0;
    constructor() { super(); ControlledWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
    terminate() { this.terminations++; }
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: ControlledWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = ControlledWorker.current;
    await bootstrapLegacy(worker);
    const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
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
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-01", generation: 2, snapshot });
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
  class IsolatedWorker extends LegacyWorker {
    static current: IsolatedWorker;
    readonly sent: Record<string, unknown>[] = [];
    constructor() { super(); IsolatedWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
    terminate() {}
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: IsolatedWorker });
  let oldHost: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  let newHost: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const oldCreating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const oldWorker = IsolatedWorker.current;
    await bootstrapLegacy(oldWorker);
    oldWorker.emit({ ...catalogWorkerFixture(protocolSnapshot(0, 0)), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot: protocolSnapshot(0, 0) });
    oldHost = await oldCreating;
    const rejectingStart = oldHost.start(() => false);
    const unexpectedStart = oldWorker.sent.find((message) => message.type === "start");
    if (unexpectedStart !== undefined) oldWorker.emit({ type: "started", generation: 1, requestId: unexpectedStart.requestId });
    await assert.rejects(rejectingStart, /消费者拒绝.*baseline/);
    assert.equal(oldWorker.sent.filter((message) => message.type === "start" || message.type === "uiFrame").length, 0);
    const newCreating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 2n);
    const newWorker = IsolatedWorker.current;
    await bootstrapLegacy(newWorker);
    newWorker.emit({ ...catalogWorkerFixture(protocolSnapshot(9, 4)), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot: protocolSnapshot(9, 4) });
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
  class PendingWorker extends LegacyWorker {
    static current: PendingWorker;
    readonly sent: Record<string, unknown>[] = [];
    terminations = 0;
    constructor() { super(); PendingWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
    terminate() { this.terminations += 1; }
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: PendingWorker });
  try {
    for (const reason of ["dispose", "fatal"]) {
      const creating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
      const worker = PendingWorker.current;
      await bootstrapLegacy(worker);
      const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
      worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
      const host = await creating;
      const date = host.civilDate!();
      const preferences = host.setPausePreferences({ pause_after_close: true, pause_before_open: false });
      assert.deepEqual(worker.sent.slice(-2).map((request) => request.requestId), [1, 2]);
      worker.emit({ type: "pausePreferencesSet", requestId: 2, generation: 1 });
      await preferences;
      let settled = false;
      const dateAssertion = assert.rejects(date, /已被销毁|TEST_FATAL/);
      void date.then(() => { settled = true; }, () => { settled = true; });
      const disposing = reason === "dispose" ? host.dispose() : undefined;
      if (reason === "fatal") {
        worker.emit({ type: "failure", code: "TEST_FATAL", where: "worker-test", message: "失败" });
        assert.equal(worker.terminations, 1);
      }
      await dateAssertion;
      assert.equal(settled, true);
      if (disposing !== undefined) await disposing;
      assert.equal(worker.terminations, 1);
      await assert.rejects(host.readSpeedMetrics(), /已被销毁|TEST_FATAL/);
    }
  } finally {
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("Worker 真实 coordinator 拒绝批次中间提交后不消费尾部、不 ACK、立即终止并取消控制", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class RejectingWorker extends LegacyWorker {
    static current: RejectingWorker;
    readonly sent: Record<string, unknown>[] = [];
    terminations = 0;
    constructor() { super(); RejectingWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
    terminate() { this.terminations++; }
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: RejectingWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = RejectingWorker.current;
    await bootstrapLegacy(worker);
    worker.emit({ ...catalogWorkerFixture(protocolSnapshot(0, 0)), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot: protocolSnapshot(0, 0) });
    host = await creating;
    let applications = 0;
    const failures: unknown[] = [];
    const coordinator = new ProtocolCoordinator({ onBaseline: () => {}, onApplied: () => { applications++; }, onFailure: (failure) => { failures.push(failure); } });
    const starting = host.start((update) => coordinator.accept(update));
    worker.emit({ type: "started", generation: 1, requestId: worker.sent.at(-1)!.requestId });
    await starting;
    const pending = assert.rejects(host.setSpeed(10), /消费者拒绝/);
    worker.emit({ type: "protocolBatch", generation: 1, deliveryId: 7, updates: [
      tickBatch([frame(1, 0, [0])], protocolSnapshot(1, 1)),
      tickBatch([frame(3, 1, [0])], protocolSnapshot(3, 2)),
      tickBatch([frame(2, 1, [0])], protocolSnapshot(2, 2)),
    ] });
    await pending;
    assert.equal(applications, 1);
    assert.equal(failures.length, 1);
    assert.equal(coordinator.status().kind, "failure");
    assert.equal(worker.sent.filter((message) => message.type === "uiFrame").length, 0);
    assert.equal(worker.terminations, 1);
    worker.emit({ type: "protocol", generation: 1, deliveryId: 8, update: tickBatch([frame(1, 0, [0])], protocolSnapshot(1, 1)) });
    assert.equal(applications, 1);
    await host.dispose();
    assert.equal(worker.terminations, 1);
  } finally {
    await host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});


test("Worker reportError 的 error 事件将完整错误交付用户回调并终止会话", async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class ReportingWorker extends LegacyWorker {
    static current: ReportingWorker;
    readonly sent: Record<string, unknown>[] = [];
    terminated = false;
    constructor() { super(); ReportingWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
    terminate() { this.terminated = true; }
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: ReportingWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = ReportingWorker.current;
    await bootstrapLegacy(worker);
    worker.emit({ ...catalogWorkerFixture(protocolSnapshot(0, 0)), type: "baseline", civilDate: "2030-01-02", generation: 1, snapshot: protocolSnapshot(0, 0) });
    host = await creating;
    const failures: unknown[] = [];
    const starting = host.start(() => true, (failure) => { failures.push(failure); });
    await Promise.resolve();
    const detail = "STEP_FATAL：原始故障；首次发送：port failure 1；再次发送：port failure 0";
    worker.dispatchEvent(Object.assign(new Event("error"), { message: detail }));
    await assert.rejects(starting, /STEP_FATAL.*原始故障.*port failure 1.*port failure 0/);
    assert.deepEqual(failures.map((value) => {
      const { code, where, message } = value as { code: string; where: string; message: string };
      return { code, where, message };
    }), [{ code: "WASM_WORKER_SCRIPT", where: "worker-host", message: `WASM Worker 脚本加载或执行失败：${detail}` }]);
    assert.equal((failures[0] as { cause: { message: string } }).cause.message, detail);
    assert.equal(worker.terminated, true);
  } finally {
    await host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

for (const shared of [true]) {
  test(`实际单股Worker ${shared ? "shared ingress" : "serial"}三variant与unknown零enqueue`, { timeout: 10000 }, async () => {
    const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
    class IntentWorker extends LegacyWorker {
      static instances: IntentWorker[] = [];
      sent: Record<string, unknown>[] = [];
      constructor() { super(); IntentWorker.instances.push(this); }
      postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
      terminate() {}
      emit(data: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data })); }
    }
    Object.defineProperty(globalThis, "Worker", { configurable: true, value: IntentWorker });
    let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
    try {
      const creating = createLegacyHost({} as never, 1n);
      const engine = IntentWorker.instances[0];
      await bootstrapLegacy(engine, false);
      engine.emit({ type: "ingress", generation: 1, token: 7, shared });
      const intake = shared ? IntentWorker.instances[1] : engine;
      if (shared) intake.emit({ type: "ingressBound", generation: 1, requestId: intake.sent[0].requestId });
      engine.emit({ type: "baseline", generation: 1, snapshot: protocolSnapshot(0, 0), civilDate: "2030-01-02",
        stockCatalog: [{ id: 0, code: "600101" }], companyCatalog: [{ id: 0, code: "C-600101", name: "公司", listed_stock: 0 }] });
      host = await creating;
      for (const [intent, expected] of [
        [{ PlaceLimit: { code: "600101", side: "Buy", price: { Fixed: "1234" }, qty: 100 } }, { PlaceLimit: { code: 0, side: "Buy", price: { Fixed: "1234" }, qty: 100 } }],
        [{ PlaceMarket: { code: "600101", side: "Sell", qty: 200 } }, { PlaceMarket: { code: 0, side: "Sell", qty: 200 } }],
        [{ Cancel: { code: "600101", id: 9 } }, { Cancel: { code: 0, id: 9 } }],
      ] as const) {
        const pending = host.submitIntent(intent);
        await Promise.resolve();
        const request = intake.sent.at(-1)!;
        assert.equal(request.type, "enqueue");
        assert.equal(request.generation, 1);
        assert.deepEqual(request.intent, expected);
        intake.emit({ type: "enqueued", generation: 1, requestId: request.requestId });
        await pending;
      }
      const count = intake.sent.length;
      for (const intent of [
        { PlaceLimit: { code: "000812", side: "Buy", price: { Fixed: "1234" }, qty: 100 } },
        { PlaceMarket: { code: "000812", side: "Sell", qty: 200 } },
        { Cancel: { code: "000812", id: 9 } },
      ] as const) await assert.rejects(host.submitIntent(intent), /000812/);
      assert.equal(intake.sent.length, count);
      const old = host.submitIntent({ Cancel: { code: "600101", id: 10 } });
      const rejected = assert.rejects(old, /过期/);
      const oldRequest = intake.sent.at(-1)!;
      engine.emit({ type: "baseline", generation: 2, civilDate: "2030-02-02",
        snapshot: { ...protocolSnapshot(0, 0), markets: { "0": market(), "1": market() }, daily_candles: { "0": [], "1": [] }, active_daily_candles: { "0": dailyCandle(), "1": dailyCandle() } },
        stockCatalog: [{ id: 0, code: "002156" }, { id: 1, code: "600101" }],
        companyCatalog: [{ id: 0, code: "C-002156", name: "芯片", listed_stock: 0 }, { id: 1, code: "C-600101", name: "公司", listed_stock: 1 }] });
      if (!shared) intake.emit({ type: "enqueued", generation: 1, requestId: oldRequest.requestId });
      await rejected;
      assert.equal(intake.sent.length, count + (shared ? 0 : 1), "shared等待间换代不得发送旧请求");
    } finally {
      await host?.dispose();
      if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
      else Object.defineProperty(globalThis, "Worker", original);
    }
  });
}

for (const transition of ["dispose", "generation"] as const) {
  test(`Worker civilDate 已回包但 continuation 前 ${transition} 必须拒绝旧结果`, { timeout: 10000 }, async () => {
    const f = fixtureW1();
    try {
      f.launch(); await drainW1(); f.ingress(); f.baseline(); await drainW1(); f.ack(f.workers[1], 1); await drainW1();
      const result = f.readOutcome();
      assert.equal(result.status, "fulfilled");
      if (result.status !== "fulfilled") throw new Error("fixture 未完成初始化");
      const pending = result.host.civilDate!();
      const rejected = assert.rejects(pending, /过期|销毁/);
      const request = f.workers[0].sent.at(-1)!;
      f.workers[0].emit({ type: "civilDate", generation: 1, requestId: request.requestId, date: "2030-01-02" });
      if (transition === "dispose") await result.host.dispose();
      else f.baseline(2);
      await rejected;
    } finally { await f.cleanup(); }
  });
}

test("WorkerHost save remains pending through 10 seconds and accepts saved at 15.202 seconds", { timeout: 10000 }, async (context) => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class SlowSaveWorker extends LegacyWorker {
    static current: SlowSaveWorker;
    readonly sent: Record<string, unknown>[] = [];
    constructor() { super(); SlowSaveWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
    terminate() {}
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: SlowSaveWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const ready = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = SlowSaveWorker.current;
    await bootstrapLegacy(worker);
    const snapshot = { seq: 42, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
    worker.emit({ type: "created", generation: 1, capabilities: { npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false }, personalTradeHistory: false } });
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
    host = await ready;
    context.mock.timers.enable({ apis: ["setTimeout"] });
    const pending = host.save({ seq: 17, settledDate: "2030-01-05" });
    const request = worker.sent.at(-1)!;
    context.mock.timers.tick(10_000);
    await Promise.resolve();
    assert.equal(request.type, "save");
    context.mock.timers.tick(5_202);
    worker.emit({ type: "saved", requestId: request.requestId, generation: 1, slot: { seq: 17 } });
    assert.deepEqual(await pending, { seq: 17 });
    const overdueSave = host.save();
    const overdueAssertion = assert.rejects(overdueSave, /Worker save 操作超时（60000ms）/);
    context.mock.timers.tick(60_000);
    await overdueAssertion;
    const sentBeforeRejectedQuery = worker.sent.length;
    const civilDate = host.civilDate;
    assert.ok(civilDate, "WorkerHost 应提供 civilDate capability");
    await assert.rejects(civilDate(), /60000ms/);
    assert.equal(worker.sent.length, sentBeforeRejectedQuery);
    await host.dispose();
    assert.equal(worker.sent.some((message) => message.type === "shutdown"), true);
  } finally {
    await host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("Worker save 超时后 scope 拒绝新请求，close 仍取消 late response", { timeout: 10000 }, async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const port = new FakeWorker();
  const requests = new WorkerRequestScope(port);
  const timedOut = requests.request({ type: "save", requestId: requests.nextRequestId(), generation: 4 }, "saved", 60_000);
  const timeoutAssertion = assert.rejects(timedOut, /Worker save 操作超时（60000ms）/);
  context.mock.timers.tick(60_000);
  await timeoutAssertion;
  assert.equal(requests.pendingCount(), 0);
  const pending = requests.request({ type: "save", requestId: requests.nextRequestId(), generation: 4 }, "saved", 60_000);
  const disposed = assert.rejects(pending, /60000ms/);
  requests.close(new Error("disposed"));
  await disposed;
  port.emit({ type: "saved", requestId: 2, generation: 4, slot: { late: true } });
  assert.equal(requests.pendingCount(), 0);
});

test("Worker save 占用期普通请求不发出也不启动 deadline，saved 后按原 10 秒 deadline 运行", { timeout: 10000 }, async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const port = new FakeWorker();
  const requests = new WorkerRequestScope(port);
  const saveId = requests.nextRequestId();
  const save = requests.request({ type: "save", requestId: saveId, generation: 4 }, "saved", 60_000);
  const queryId = requests.nextRequestId();
  const query = requests.request({ type: "civilDate", requestId: queryId, generation: 4 }, "civilDate");
  const queryAssertion = assert.doesNotReject(query);
  context.mock.timers.tick(10_000);
  assert.deepEqual(port.sent, [{ type: "save", requestId: saveId, generation: 4 }]);
  context.mock.timers.tick(5_202);
  port.emit({ type: "saved", requestId: saveId, generation: 4, slot: {} });
  await save;
  assert.deepEqual(port.sent, [
    { type: "save", requestId: saveId, generation: 4 },
    { type: "civilDate", requestId: queryId, generation: 4 },
  ]);
  context.mock.timers.tick(9_999);
  port.emit({ type: "civilDate", requestId: queryId, generation: 4, date: "2030-01-05" });
  await queryAssertion;
  assert.equal(requests.pendingCount(), 0);
});

test("Worker save operationError 显式拒绝当前保存并按 FIFO 放行下一保存边界", { timeout: 10000 }, async () => {
  const port = new FakeWorker();
  const requests = new WorkerRequestScope(port);
  const firstId = requests.nextRequestId();
  const first = requests.request({ type: "save", requestId: firstId, generation: 4 }, "saved", 60_000);
  const firstAssertion = assert.rejects(first, /存档写入失败/);
  const queryId = requests.nextRequestId();
  const query = requests.request({ type: "civilDate", requestId: queryId, generation: 4 }, "civilDate");
  const secondSaveId = requests.nextRequestId();
  const secondSave = requests.request({ type: "save", requestId: secondSaveId, generation: 4 }, "saved", 60_000);
  const secondQueryId = requests.nextRequestId();
  const secondQuery = requests.request({ type: "speedMetrics", requestId: secondQueryId, generation: 4 }, "speedMetrics");
  assert.equal(port.sent.length, 1);
  port.emit({ type: "operationError", requestId: firstId, generation: 4, message: "存档写入失败" });
  await firstAssertion;
  assert.equal((port.sent[1] as { requestId: number }).requestId, queryId);
  port.emit({ type: "civilDate", requestId: queryId, generation: 4, date: "2030-01-05" });
  await query;
  assert.equal((port.sent[2] as { requestId: number }).requestId, secondSaveId);
  assert.equal(port.sent.length, 3);
  port.emit({ type: "saved", requestId: secondSaveId, generation: 4, slot: {} });
  await secondSave;
  assert.equal((port.sent[3] as { requestId: number }).requestId, secondQueryId);
  port.emit({ type: "speedMetrics", requestId: secondQueryId, generation: 4, metrics: {} });
  await secondQuery;
});

test("Worker save 超时会拒绝整队且 poison scope 仍允许显式 shutdown", { timeout: 10000 }, async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const port = new FakeWorker();
  const requests = new WorkerRequestScope(port);
  const saveId = requests.nextRequestId();
  const save = requests.request({ type: "save", requestId: saveId, generation: 4 }, "saved", 60_000);
  const saveAssertion = assert.rejects(save, /Worker save 操作超时（60000ms）/);
  const queuedId = requests.nextRequestId();
  const queued = requests.request({ type: "civilDate", requestId: queuedId, generation: 4 }, "civilDate");
  const queuedAssertion = assert.rejects(queued, /60000ms/);
  context.mock.timers.tick(60_000);
  await Promise.all([saveAssertion, queuedAssertion]);
  assert.equal(requests.pendingCount(), 0);
  port.emit({ type: "saved", requestId: saveId, generation: 4, slot: {} });
  assert.equal(port.sent.length, 1);
  requests.beginClosing("shutdown", new Error("closing"));
  const shutdownId = requests.nextRequestId();
  const shutdown = requests.request({ type: "shutdown", requestId: shutdownId, generation: 4 }, "shutdownCompleted");
  assert.equal((port.sent[1] as { type: string }).type, "shutdown");
  port.emit({ type: "shutdownCompleted", requestId: shutdownId, generation: 4 });
  await shutdown;
});

test("Worker close 与 beginClosing 取消 save 和排队请求，晚到 saved 不会迟发", { timeout: 10000 }, async () => {
  for (const transition of ["close", "beginClosing"] as const) {
    const port = new FakeWorker();
    const requests = new WorkerRequestScope(port);
    const saveId = requests.nextRequestId();
    const save = requests.request({ type: "save", requestId: saveId, generation: 4 }, "saved", 60_000);
    const saveAssertion = assert.rejects(save, /closed/);
    const queued = requests.request({ type: "civilDate", requestId: requests.nextRequestId(), generation: 4 }, "civilDate");
    const queuedAssertion = assert.rejects(queued, /closed/);
    if (transition === "close") requests.close(new Error("closed"));
    else requests.beginClosing("shutdown", new Error("closed"));
    await Promise.all([saveAssertion, queuedAssertion]);
    port.emit({ type: "saved", requestId: saveId, generation: 4, slot: {} });
    if (transition === "beginClosing") {
      const shutdownId = requests.nextRequestId();
      const shutdown = requests.request({ type: "shutdown", requestId: shutdownId, generation: 4 }, "shutdownCompleted");
      port.emit({ type: "shutdownCompleted", requestId: shutdownId, generation: 4 });
      await shutdown;
    }
    assert.deepEqual(port.sent.map((request) => (request as { type: string }).type), transition === "close" ? ["save"] : ["save", "shutdown"]);
    assert.equal(requests.pendingCount(), 0);
  }
});

test("Worker generation 前进取消旧 save 与队列，同代 baseline 保留在途请求", { timeout: 10000 }, async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class GenerationWorker extends LegacyWorker {
    static current: GenerationWorker;
    readonly sent: Record<string, unknown>[] = [];
    constructor() { super(); GenerationWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.prepare(value); this.sent.push(value); }
    terminate() {}
    emit(value: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data: value })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: GenerationWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createLegacyHost({} as Parameters<typeof createWorkerHost>[0], 1n);
    const worker = GenerationWorker.current;
    await bootstrapLegacy(worker);
    const snapshot = { seq: 0, tick: 0, day: 0, phase: "Continuous", mirror_origin: mirrorOrigin(0), markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-01", generation: 1, snapshot });
    host = await creating;
    const save = host.save();
    const saveAssertion = assert.rejects(save, /generation/);
    const query = host.civilDate!();
    const queryAssertion = assert.rejects(query, /generation/);
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-02", generation: 2, snapshot });
    context.mock.timers.tick(60_000);
    await Promise.all([saveAssertion, queryAssertion]);
    assert.deepEqual(worker.sent.filter((request) => request.type === "save" || request.type === "civilDate").map((request) => request.type), ["save"]);

    const nextSave = host.save();
    const sameGenerationQuery = host.civilDate!();
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-03", generation: 2, snapshot });
    const sameRequest = worker.sent.at(-1)!;
    assert.equal(sameRequest.type, "save");
    assert.ok(typeof sameRequest.requestId === "number", "save request 应携带数字 requestId");
    const sameRequestId = sameRequest.requestId;
    worker.emit({ type: "saved", requestId: sameRequestId, generation: 2, slot: {} });
    await nextSave;
    worker.emit({ type: "civilDate", requestId: sameRequestId + 1, generation: 2, date: "2030-01-03" });
    await sameGenerationQuery;

    let newGenerationQuery: Promise<unknown> | undefined;
    const starting = host.start((update) => {
      if (update.generation === "3" || update.generation === "4") newGenerationQuery = host!.civilDate!().then((value) => ({ kind: "value", value }), (error: unknown) => ({ kind: "error", error }));
      return true;
    });
    const startRequest = worker.sent.at(-1)!;
    worker.emit({ type: "started", requestId: startRequest.requestId, generation: 2 });
    await starting;
    const oldSave = host.save();
    const oldSaveAssertion = assert.rejects(oldSave, /generation/);
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "baseline", civilDate: "2030-01-04", generation: 3, snapshot });
    await oldSaveAssertion;
    assert.ok(newGenerationQuery, "同步 baseline callback 应创建新代查询");
    const newGenerationRequest = worker.sent.at(-1)!;
    assert.deepEqual(newGenerationRequest, { type: "civilDate", requestId: newGenerationRequest.requestId, generation: 3 });
    worker.emit({ type: "civilDate", requestId: newGenerationRequest.requestId, generation: 3, date: "2030-01-04" });
    const newGenerationOutcome = await newGenerationQuery;
    assert.equal((newGenerationOutcome as { kind: string }).kind, "value");

    const loading = host.load(currentSaveFixture());
    const restoreRequest = worker.sent.at(-1)!;
    worker.emit({ ...catalogWorkerFixture(snapshot), type: "restored", civilDate: "2030-01-05", requestId: restoreRequest.requestId, generation: 3, nextGeneration: 4, snapshot });
    await loading;
    assert.ok(newGenerationQuery, "restore baseline callback 应创建新代查询");
    const restoreGenerationRequest = worker.sent.at(-1)!;
    assert.deepEqual(restoreGenerationRequest, { type: "civilDate", requestId: restoreGenerationRequest.requestId, generation: 4 });
    worker.emit({ type: "civilDate", requestId: restoreGenerationRequest.requestId, generation: 4, date: "2030-01-05" });
    const restoreGenerationOutcome = await newGenerationQuery;
    assert.equal((restoreGenerationOutcome as { kind: string }).kind, "value");
  } finally {
    await host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("Worker 无 save 时普通请求仍按 10 秒超时", { timeout: 10000 }, async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const port = new FakeWorker();
  const requests = new WorkerRequestScope(port);
  const query = requests.request({ type: "civilDate", requestId: requests.nextRequestId(), generation: 4 }, "civilDate");
  const assertion = assert.rejects(query, /Worker civilDate 操作超时（10000ms）/);
  context.mock.timers.tick(9_999);
  assert.equal(requests.pendingCount(), 1);
  context.mock.timers.tick(1);
  await assertion;
  assert.equal(requests.pendingCount(), 0);
});

test("Worker 排队 save postMessage 同步失败显式拒绝并放行后续 FIFO 请求", { timeout: 10000 }, async () => {
  class SavePostFailurePort extends FakeWorker {
    failed = false;
    postMessage(message: unknown): void {
      if (!this.failed && (message as { type: string }).type === "save" && this.sent.length > 0) {
        this.failed = true;
        throw new Error("DataCloneError: queued save payload");
      }
      super.postMessage(message);
    }
  }
  const port = new SavePostFailurePort();
  const requests = new WorkerRequestScope(port);
  const firstId = requests.nextRequestId();
  const first = requests.request({ type: "save", requestId: firstId, generation: 4 }, "saved", 60_000);
  const secondId = requests.nextRequestId();
  const second = requests.request({ type: "save", requestId: secondId, generation: 4 }, "saved", 60_000);
  const secondAssertion = assert.rejects(second, /DataCloneError: queued save payload/);
  const queryId = requests.nextRequestId();
  const query = requests.request({ type: "civilDate", requestId: queryId, generation: 4 }, "civilDate");
  assert.deepEqual(port.sent, [{ type: "save", requestId: firstId, generation: 4 }]);
  port.emit({ type: "saved", requestId: firstId, generation: 4, slot: {} });
  await first;
  await secondAssertion;
  assert.deepEqual(port.sent, [
    { type: "save", requestId: firstId, generation: 4 },
    { type: "civilDate", requestId: queryId, generation: 4 },
  ]);
  port.emit({ type: "civilDate", requestId: queryId, generation: 4, date: "2030-01-05" });
  await query;
  assert.equal(requests.pendingCount(), 0);
});
