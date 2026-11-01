import assert from "node:assert/strict";
import test from "node:test";
import { drain, fixture } from "./worker-host-w1.test-support.ts";
import { CompanyWriteRejectedError } from "./company-write.ts";
import { catalogWorkerFixture } from "./catalog-test-fixture.ts";
import { snapshot } from "./protocol-test-fixtures.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";

function protocolBaseline(generation: number) { return { ...catalogWorkerFixture(snapshot(generation, generation)), type: "baseline", generation, civilDate: "2030-01-02", snapshot: snapshot(generation, generation) }; }

async function withHost(body: (host: Awaited<ReturnType<ReturnType<typeof fixture>["launch"]>>, f: ReturnType<typeof fixture>) => Promise<void>) {
  const f = fixture();
  try {
    f.launch(); await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
    const initialized = f.readOutcome();
    assert.equal(initialized.status, "fulfilled");
    if (initialized.status !== "fulfilled") throw new Error("健康 Host fixture 未完成初始化");
    for (const name of ["proposePlayerCompanyAction", "enqueueReportCorrection", "cancelReportCorrection"]) assert.equal(typeof Reflect.get(initialized.host, name), "function");
    await body(initialized.host, f);
  } finally { await f.cleanup(); }
}

const proposal = { CashDividend: { company: 0, gross_per_share: "5" } };
const receipt = { account: "0", company: 0, kind: "CashDividend", event_id: "synthetic-approved", approved_on: "2030-01-02" };
const failure = { kind: "PlayerProposal", data: { account: "0", company: 0, kind: "CashDividend", day: "2030-01-02", classification: "BusinessCondition", reason: "具体持仓拒绝" } };

test("Worker Host 真实三写透传及 typed 拒绝后继续使用同一会话", { timeout: 10000 }, async () => {
  await withHost(async (host, f) => {
    const engine = f.workers[0];
    const controlOnPost = engine.onPost;
    assert.ok(controlOnPost, "健康 fixture 必须保留 shutdown 控制消息处理器");
    engine.onPost = (raw, transfer) => {
      if (raw.type === "shutdown" || raw.type === "quiesceIngress") { controlOnPost(raw, transfer); return; }
      if (raw.type === "proposePlayerCompanyAction") engine.emit({ type: "playerCompanyActionAcknowledged", requestId: raw.requestId, generation: raw.generation, result: { receipt } });
      if (raw.type === "enqueueReportCorrection" || raw.type === "cancelReportCorrection") engine.emit({ type: raw.type === "enqueueReportCorrection" ? "reportCorrectionEnqueueAcknowledged" : "reportCorrectionCancelAcknowledged", requestId: raw.requestId, generation: raw.generation, result: { operation_id: "synthetic-correction" } });
    };
    assert.deepEqual(await host.proposePlayerCompanyAction(proposal), receipt);
    const request = { operation_id: "synthetic-correction", company: 0, supersedes: 7, entries: [], reason: "synthetic transport fixture" };
    assert.deepEqual(await host.enqueueReportCorrection(request), { operation_id: request.operation_id });
    assert.deepEqual(await host.cancelReportCorrection(request.operation_id), { operation_id: request.operation_id });
    const sent = engine.sent.filter(raw => ["proposePlayerCompanyAction", "enqueueReportCorrection", "cancelReportCorrection"].includes(String(raw.type)));
    assert.deepEqual(sent.map(({ requestId, ...raw }) => { assert.ok(Number.isSafeInteger(requestId)); return raw; }), [
      { type: "proposePlayerCompanyAction", generation: 1, proposal }, { type: "enqueueReportCorrection", generation: 1, request }, { type: "cancelReportCorrection", generation: 1, operation_id: request.operation_id },
    ]);
    engine.onPost = (raw, transfer) => {
      if (raw.type === "shutdown" || raw.type === "quiesceIngress") { controlOnPost(raw, transfer); return; }
      engine.emit({ type: "operationError", requestId: raw.requestId, generation: raw.generation, message: "具体持仓拒绝", failure });
    };
    await assert.rejects(host.proposePlayerCompanyAction(proposal), error => {
      assert.ok(error instanceof CompanyWriteRejectedError);
      assert.deepEqual(error.cause, failure);
      return true;
    });
    assert.equal(engine.terminations, 0);
    engine.onPost = (raw, transfer) => {
      if (raw.type === "shutdown" || raw.type === "quiesceIngress") { controlOnPost(raw, transfer); return; }
      engine.emit({ type: "playerCompanyActionAcknowledged", requestId: raw.requestId, generation: raw.generation, result: { receipt } });
    };
    assert.deepEqual(await host.proposePlayerCompanyAction(proposal), receipt);
  });
});

for (const settlement of ["success", "rejection"] as const) for (const replacement of ["same-generation", "next-generation", "dispose"] as const) {
  test(`Worker 三写 ${replacement} 后拒绝旧 ${settlement} 并保原 cause`, { timeout: 10000 }, async () => {
    await withHost(async (host, f) => {
      const engine = f.workers[0];
      const listenersBeforeRequest = engine.listenerCount();
      const timersBeforeRequest = f.clock.tasks.size;
      const sentBeforeRequest = engine.sent.length;
      const pending = host.proposePlayerCompanyAction(proposal);
      const result = pending.then(value => ({ ok: true as const, value }), (error: unknown) => ({ ok: false as const, error }));
      await drain();
      const sent = engine.sent.at(-1)!;
      if (replacement === "next-generation") {
        f.baseline(2);
        const canceled = await result;
        assert.equal(canceled.ok, false, "generation 前进必须先立即取消旧写");
        if (canceled.ok) throw new Error("换代必须先取消旧写");
        assert.ok(canceled.error instanceof Error);
        assert.match(canceled.error.message, /过期/);
        const cancellation = canceled.error.cause;
        assert.ok(cancellation instanceof Error);
        assert.match(cancellation.message, /generation 前进已取消/);
        assert.equal(engine.sent.length, sentBeforeRequest + 1, "换代取消不得补发公司写");
        assert.equal(engine.listenerCount(), listenersBeforeRequest, "取消后必须移除逐请求 message listener");
        assert.equal(f.clock.tasks.size, timersBeforeRequest, "取消后必须清除逐请求 timeout");
        const sentBeforeLateResponse = engine.sent.length;
        const listenersBeforeLateResponse = engine.listenerCount();
        const timersBeforeLateResponse = f.clock.tasks.size;
        engine.emit(settlement === "success"
          ? { type: "playerCompanyActionAcknowledged", requestId: sent.requestId, generation: sent.generation, result: { receipt } }
          : { type: "operationError", requestId: sent.requestId, generation: sent.generation, message: "具体持仓拒绝", failure });
        assert.equal(await result, canceled, "迟到响应不得改变已完成的取消结果");
        assert.equal(canceled.error.cause, cancellation, "迟到响应不得替换真实取消 cause");
        assert.equal(engine.sent.length, sentBeforeLateResponse);
        assert.equal(engine.listenerCount(), listenersBeforeLateResponse);
        assert.equal(f.clock.tasks.size, timersBeforeLateResponse);
        assert.equal(engine.terminations, 0);
        return;
      }
      if (replacement === "same-generation") engine.emitRaw(protocolBaseline(1));
      engine.emitRaw(settlement === "success"
        ? { type: "playerCompanyActionAcknowledged", requestId: sent.requestId, generation: sent.generation, result: { receipt } }
        : { type: "operationError", requestId: sent.requestId, generation: sent.generation, message: "具体持仓拒绝", failure });
      if (replacement === "dispose") await host.dispose();
      const outcome = await result;
      assert.equal(outcome.ok, false);
      if (outcome.ok) throw new Error("旧确认不能成功");
      assert.ok(outcome.error instanceof Error);
      assert.match(outcome.error.message, /过期|销毁/);
      if (settlement === "rejection") {
        assert.ok(outcome.error.cause instanceof CompanyWriteRejectedError);
        assert.deepEqual(outcome.error.cause.cause, failure);
      }
    });
  });
}

test("Worker 已接收的 typed 公司写拒绝在换代后仍保留完整 failure", { timeout: 10000 }, async () => {
  await withHost(async (host, f) => {
    const engine = f.workers[0];
    const pending = host.proposePlayerCompanyAction(proposal).then(value => ({ ok: true as const, value }), (error: unknown) => ({ ok: false as const, error }));
    await drain();
    const sent = engine.sent.at(-1)!;
    engine.emitRaw({ type: "operationError", requestId: sent.requestId, generation: sent.generation, message: "具体持仓拒绝", failure });
    const next = protocolBaseline(2);
    engine.emitRaw(next);
    const outcome = await pending;
    assert.equal(outcome.ok, false);
    if (outcome.ok) throw new Error("typed 公司写拒绝不能成功");
    assert.ok(outcome.error instanceof Error);
    assert.match(outcome.error.message, /过期/);
    assert.ok(outcome.error.cause instanceof CompanyWriteRejectedError);
    assert.deepEqual(outcome.error.cause.cause, failure);
  });
});

test("Worker fatal 拒绝在途和后续公司写，不因可见基线仍在而继续发送", { timeout: 10000 }, async () => {
  await withHost(async (host, f) => {
    const pending = host.proposePlayerCompanyAction(proposal);
    const rejected = assert.rejects(pending);
    f.workers[0].emit({ type: "failure", generation: 1, code: "STEP_FATAL", message: "真实 Step 形状的 fatal", context: { tick: 7 } });
    await rejected;
    const sent = f.workers[0].sent.length;
    await assert.rejects(host.proposePlayerCompanyAction(proposal), /健康|销毁|过期/);
    assert.equal(f.workers[0].sent.length, sent);
  });
});

for (const settlement of ["success", "rejection"] as const) {
  test(`Worker restore 在途新 ingress 先行时旧写 ${settlement} 不跨 authority`, { timeout: 10000 }, async () => {
    await withHost(async (host, f) => {
      const engine = f.workers[0];
      const writing = host.proposePlayerCompanyAction(proposal).then(value => ({ ok: true as const, value }), (error: unknown) => ({ ok: false as const, error }));
      await drain();
      const sent = engine.sent.at(-1)!;
      const loading = host.load(currentSaveFixture()).then(() => ({ ok: true as const }), (error: unknown) => ({ ok: false as const, error }));
      f.ingress(2, 202);
      engine.emit(settlement === "success"
        ? { type: "playerCompanyActionAcknowledged", requestId: sent.requestId, generation: sent.generation, result: { receipt } }
        : { type: "operationError", requestId: sent.requestId, generation: sent.generation, message: "旧提案拒绝", failure });
      const outcome = await writing;
      assert.equal(outcome.ok, false);
      if (outcome.ok) throw new Error("restore 在途不得接受旧写确认");
      assert.ok(outcome.error instanceof Error);
      assert.match(outcome.error.message, /过期|restore/);
      if (settlement === "rejection") assert.ok(outcome.error.cause instanceof CompanyWriteRejectedError);
      const before = engine.sent.length;
      await assert.rejects(host.proposePlayerCompanyAction(proposal), /健康|restore|过期/);
      assert.equal(engine.sent.length, before);
      await host.dispose();
      assert.equal((await loading).ok, false);
    });
  });
}

for (const method of ["proposePlayerCompanyAction", "enqueueReportCorrection", "cancelReportCorrection"] as const) {
  test(`健康 WorkerHost 注册 ${method} 真实入口`, { timeout: 10000 }, async () => {
    const f = fixture();
    try {
      f.launch(); await drain(); f.ingress(); f.baseline(); await drain();
      f.ack(f.workers[1], 1); await drain();
      const initialized = f.readOutcome();
      assert.equal(initialized.status, "fulfilled", "必须先通过真实 facade bootstrap 与 ingress ACK");
      if (initialized.status !== "fulfilled") throw new Error("健康 Host fixture 未完成初始化");
      assert.equal(f.configured, 1);
      assert.equal(f.failures.length, 0);
      assert.equal(f.workers.every(worker => worker.terminations === 0), true);
      assert.equal(typeof Reflect.get(initialized.host, method), "function", `实际 WorkerHost 必须提供 ${method}`);
    } finally { await f.cleanup(); }
  });
}

test("Worker Host 认购透传请求并核对本人 typed 拒绝", { timeout: 10000 }, async () => {
  await withHost(async (host, f) => {
    const engine = f.workers[0];
    const control = engine.onPost!;
    engine.onPost = (raw, transfer) => {
      if (raw.type === "shutdown" || raw.type === "quiesceIngress") { control(raw, transfer); return; }
      engine.emit({ type: "rightsSubscriptionAcknowledged", requestId: raw.requestId, generation: raw.generation, result: { event_id: "r1", requested_shares: "5", submitted_on: "2030-01-02" } });
    };
    assert.equal(typeof host.subscribeRightsOffering, "function");
    assert.deepEqual(await host.subscribeRightsOffering!("r1", "5"), { event_id: "r1", requested_shares: "5", submitted_on: "2030-01-02" });
    const sent = engine.sent.at(-1)!;
    assert.deepEqual({ type: sent.type, generation: sent.generation, event_id: sent.event_id, requested_shares: sent.requested_shares }, { type: "subscribeRightsOffering", generation: 1, event_id: "r1", requested_shares: "5" });
    const ownFailure = { kind: "RightsSubscription", data: { account: "0", event_id: "r1", classification: "BusinessCondition", reason: "拒绝" } };
    engine.onPost = raw => engine.emit({ type: "operationError", requestId: raw.requestId, generation: raw.generation, message: "拒绝", failure: ownFailure });
    await assert.rejects(host.subscribeRightsOffering!("r1", "5"), error => error instanceof CompanyWriteRejectedError && assert.deepEqual(error.failure, ownFailure) === undefined);
    engine.onPost = raw => engine.emit({ type: "operationError", requestId: raw.requestId, generation: raw.generation, message: "拒绝", failure: { kind: "RightsSubscription", data: { account: "1", event_id: "r1", classification: "BusinessCondition", reason: "拒绝" } } });
    await assert.rejects(host.subscribeRightsOffering!("r1", "5"), AggregateError);
    engine.onPost = control;
  });
});

for (const transition of ["generation", "restore", "dispose"] as const) {
  test(`Worker 认购在 ${transition} 后拒绝迟到的确认`, { timeout: 10000 }, async () => {
    await withHost(async (host, f) => {
      const engine = f.workers[0];
      const pending = host.subscribeRightsOffering!("r1", "5").then(() => null, (error: unknown) => error);
      await drain();
      const sent = engine.sent.at(-1)!;
      let loading: Promise<unknown> | undefined;
      if (transition === "generation") f.baseline(2);
      if (transition === "restore") { loading = host.load(currentSaveFixture()).catch(error => error); f.ingress(2, 202); }
      if (transition === "dispose") await host.dispose();
      engine.emit({ type: "rightsSubscriptionAcknowledged", requestId: sent.requestId, generation: sent.generation, result: { event_id: "r1", requested_shares: "5", submitted_on: "2030-01-02" } });
      const error = await pending;
      assert.ok(error instanceof Error);
      assert.match(error.message, /过期|销毁|restore/);
      if (loading !== undefined) { await host.dispose(); await loading; }
    });
  });
}

test("同代 runtime 刷新不使在途公司写回执失效", { timeout: 10000 }, async () => {
  await withHost(async (host, f) => {
    const engine = f.workers[0];
    const writing = host.proposePlayerCompanyAction(proposal);
    await drain();
    const write = engine.sent.at(-1)!;
    const flushing = host.flushRuntimeSync!();
    await drain();
    const flush = engine.sent.at(-1)!;
    engine.emit({ ...catalogWorkerFixture(snapshot(0, 0)), type: "runtimeSyncFlushed", requestId: flush.requestId, generation: 1, snapshot: snapshot(0, 0), civilDate: "2030-01-02" });
    await flushing;
    engine.emit({ type: "playerCompanyActionAcknowledged", requestId: write.requestId, generation: 1, result: { receipt } });
    assert.deepEqual(await writing, receipt);
  });
});
