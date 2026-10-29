import assert from "node:assert/strict";
import test from "node:test";
import { drain, fixture } from "./worker-host-w1.test-support.ts";
import type { Host } from "./worker-host-w1.test-support.ts";
import type { WorkerHandle } from "./worker-host-w1.test-support.ts";
import { snapshot as protocolSnapshot } from "./protocol-test-fixtures.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";
import type { CompanyWriteCommands } from "./company-write.ts";

async function readyHost(): Promise<{ readonly f: ReturnType<typeof fixture>; readonly host: Host; readonly engine: WorkerHandle; readonly ingress: WorkerHandle }> {
  const f = fixture();
  try {
    f.launch();
    await drain();
    f.ingress();
    f.baseline();
    await drain();
    f.ack(f.workers[1], 1);
    await drain();
    const result = f.readOutcome();
    assert.equal(result.status, "fulfilled");
    if (result.status !== "fulfilled") throw new Error("W1 host fixture 未完成初始化");
    return { f, host: result.host, engine: f.workers[0], ingress: f.workers[1] };
  } catch (error) {
    await f.cleanup();
    throw error;
  }
}

function holdShutdown(engine: WorkerHandle): { readonly request: () => Record<string, unknown>; readonly acknowledge: () => void } {
  const previous = engine.onPost;
  let shutdown: Record<string, unknown> | undefined;
  engine.onPost = (value, transfer) => {
    if (value.type === "shutdown") { shutdown = value; return; }
    previous?.(value, transfer);
  };
  return {
    request() {
      if (shutdown === undefined) throw new Error("dispose 未发送 shutdown 请求");
      return shutdown;
    },
    acknowledge() {
      const request = this.request();
      engine.emit({ type: "shutdownCompleted", requestId: request.requestId, generation: request.generation, report: { kind: "success", handle: 7, runtime: null } });
    },
  };
}

async function closeAfterResponse(engine: WorkerHandle, host: Host): Promise<() => Promise<void>> {
  const shutdown = holdShutdown(engine);
  const disposing = host.dispose();
  const request = shutdown.request();
  assert.equal(request.type, "shutdown");
  let finished = false;
  return async () => {
    if (finished) return;
    finished = true;
    shutdown.acknowledge();
    await disposing;
  };
}

test("submitIntent 入队 ACK 已回包时开始关闭，旧 continuation 必须拒绝", { timeout: 10000 }, async () => {
  const { f, host, engine, ingress } = await readyHost();
  let finish: (() => Promise<void>) | undefined;
  try {
    const intent = { Cancel: { code: "000812", id: 7 } } as Parameters<typeof host.submitIntent>[0];
    const accepted = host.submitIntent(intent);
    await Promise.resolve();
    const firstEnqueue = ingress.sent.at(-1)!;
    ingress.emit({ type: "enqueued", requestId: firstEnqueue.requestId, generation: firstEnqueue.generation });
    await accepted;

    const pending = host.submitIntent(intent);
    await Promise.resolve();
    const enqueue = ingress.sent.at(-1)!;
    assert.equal(enqueue.type, "enqueue");
    ingress.emit({ type: "enqueued", requestId: enqueue.requestId, generation: enqueue.generation });
    finish = await closeAfterResponse(engine, host);
    await assert.rejects(pending, /过期|关闭/);
  } finally { await finish?.(); await f.cleanup(); }
});

test("civilDate 响应已回包时开始关闭，旧查询 continuation 必须拒绝", { timeout: 10000 }, async () => {
  const { f, host, engine } = await readyHost();
  let finish: (() => Promise<void>) | undefined;
  try {
    const before = host.civilDate!();
    const request = engine.sent.at(-1)!;
    engine.emit({ type: "civilDate", requestId: request.requestId, generation: request.generation, date: "2030-01-02" });
    assert.equal(await before, "2030-01-02", "关闭前有效查询应正常成功");

    const pending = host.civilDate!();
    const lateRequest = engine.sent.at(-1)!;
    engine.emit({ type: "civilDate", requestId: lateRequest.requestId, generation: lateRequest.generation, date: "2030-01-03" });
    finish = await closeAfterResponse(engine, host);
    await assert.rejects(pending, /过期|关闭/);
  } finally { await finish?.(); await f.cleanup(); }
});

test("refreshBaseline 响应已回包时开始关闭，不得重装 baseline 或调用 callback", { timeout: 10000 }, async () => {
  const { f, host, engine } = await readyHost();
  const delivered: unknown[] = [];
  let finish: (() => Promise<void>) | undefined;
  try {
    const starting = host.start((update) => { delivered.push(update); });
    const started = engine.sent.at(-1)!;
    engine.emit({ type: "started", requestId: started.requestId, generation: started.generation });
    await starting;
    const { catalogWorkerFixture } = await import("./catalog-test-fixture.ts");
    const normalRefresh = host.refreshBaseline();
    const normalRequest = engine.sent.at(-1)!;
    const firstSnapshot = protocolSnapshot(2, 2);
    engine.emit({ ...catalogWorkerFixture(firstSnapshot), type: "refreshed", requestId: normalRequest.requestId, generation: normalRequest.generation, civilDate: "2030-01-03", snapshot: firstSnapshot });
    await normalRefresh;
    const initialCount = delivered.length;

    const pending = host.refreshBaseline();
    const refresh = engine.sent.at(-1)!;
    const snapshot = protocolSnapshot(3, 3);
    engine.emit({ ...catalogWorkerFixture(snapshot), type: "refreshed", requestId: refresh.requestId, generation: refresh.generation, civilDate: "2030-01-04", snapshot });
    finish = await closeAfterResponse(engine, host);
    await assert.rejects(pending, /过期|关闭/);
    assert.equal(delivered.length, initialCount, "关闭期不得调用 baseline callback");
    assert.equal(host.snapshot().tick, 2, "关闭期不得安装已回包的 refresh baseline");
    await finish();
    finish = undefined;
    assert.throws(() => host.snapshot(), /快照尚未就绪/, "关闭完成后不得残留 refresh baseline");
  } finally { await finish?.(); await f.cleanup(); }
});


test("Worker company write 已回包时开始关闭，旧确认不得成功", { timeout: 10000 }, async () => {
  const { f, host, engine } = await readyHost();
  let finish: (() => Promise<void>) | undefined;
  try {
    const proposal = { CashDividend: { company: 0, gross_per_share: "5" } };
    const pending = (host as Host & CompanyWriteCommands).proposePlayerCompanyAction(proposal);
    const request = engine.sent.at(-1)!;
    engine.emit({ type: "playerCompanyActionAcknowledged", requestId: request.requestId, generation: request.generation, result: { receipt: { account: "0", company: 0, kind: "CashDividend", event_id: "synthetic-approved", approved_on: "2030-01-02" } } });
    finish = await closeAfterResponse(engine, host);
    await assert.rejects(pending, /过期|销毁/);
  } finally { await finish?.(); await f.cleanup(); }
});

test("关闭触发的 restore cancellation 不得伪装 fatal 或中断 shutdown", { timeout: 10000 }, async () => {
  const { f, host, engine } = await readyHost();
  let finish: (() => Promise<void>) | undefined;
  try {
    const loading = host.load(currentSaveFixture());
    const rejected = assert.rejects(loading, /关闭|取消/);
    f.ingress(2, 202);
    finish = await closeAfterResponse(engine, host);
    await rejected;
    assert.equal(f.failures.length, 0, "shutdown 自身取消 restore 不得重复触发 fatal");
    await finish();
    finish = undefined;
    assert.equal(f.failures.length, 0);
  } finally { await finish?.(); await f.cleanup(); }
});


test("真实 ingress bind operationError 在关闭 continuation 前到达仍报告 fatal", { timeout: 10000 }, async () => {
  const { f, host } = await readyHost();
  try {
    const loading = host.load(currentSaveFixture());
    const loadRejected = assert.rejects(loading, /关闭|取消/);
    f.ingress(2, 202);
    const binding = f.workers[1].sent.findLast((request) => request.type === "bindIngress" && request.generation === 2)!;
    f.workers[1].emit({ type: "operationError", requestId: binding.requestId, generation: binding.generation, message: "真实 ingress bind 故障" });
    const disposing = host.dispose();
    await loadRejected;
    const disposal = await disposing.then(() => ({ ok: true as const }), (error: unknown) => ({ ok: false as const, error }));
    await drain();
    assert.equal(disposal.ok, false, "真实 bind fatal 必须保留在 disposal 错误链中");
    if (disposal.ok) throw new Error("真实 bind fatal 未保留");
    const errorMessages = (error: unknown): string[] => {
      if (!(error instanceof Error)) return [String(error)];
      const nested = error instanceof AggregateError ? error.errors.flatMap(errorMessages) : [];
      const cause = error.cause === undefined ? [] : errorMessages(error.cause);
      return [error.message, ...nested, ...cause];
    };
    assert.match(errorMessages(disposal.error).join(" "), /WASM_INGRESS_BIND/);
    assert.match(errorMessages(disposal.error).join(" "), /真实 ingress bind 故障/);
  } finally { await f.cleanup(/shutdown|释放/); }
});


test("currentResult 指标响应已回包时开始关闭，旧查询 continuation 必须拒绝", { timeout: 10000 }, async () => {
  const { f, host, engine } = await readyHost();
  let finish: (() => Promise<void>) | undefined;
  try {
    const pending = host.readSpeedMetrics();
    const request = engine.sent.at(-1)!;
    engine.emit({ type: "speedMetrics", requestId: request.requestId, generation: request.generation, metrics: {
      requested: { mode: "fixed", multiplier: 60 }, actual_multiplier: 60, sample_duration_ms: 1000, sample_ticks: 60, running: true,
    } });
    finish = await closeAfterResponse(engine, host);
    await assert.rejects(pending, /过期|销毁/);
  } finally { await finish?.(); await f.cleanup(); }
});
