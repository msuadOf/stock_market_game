import assert from "node:assert/strict";
import test from "node:test";
import { drain, fixture } from "./worker-host-w1.test-support.ts";
import { CompanyWriteRejectedError } from "./company-write.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";

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
    engine.onPost = raw => {
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
    engine.onPost = raw => engine.emit({ type: "operationError", requestId: raw.requestId, generation: raw.generation, message: "具体持仓拒绝", failure });
    await assert.rejects(host.proposePlayerCompanyAction(proposal), error => {
      assert.ok(error instanceof CompanyWriteRejectedError);
      assert.deepEqual(error.cause, failure);
      return true;
    });
    assert.equal(engine.terminations, 0);
    engine.onPost = raw => engine.emit({ type: "playerCompanyActionAcknowledged", requestId: raw.requestId, generation: raw.generation, result: { receipt } });
    assert.deepEqual(await host.proposePlayerCompanyAction(proposal), receipt);
  });
});

for (const settlement of ["success", "rejection"] as const) for (const replacement of ["same-generation", "next-generation", "dispose"] as const) {
  test(`Worker 三写 ${replacement} 后拒绝旧 ${settlement} 并保原 cause`, { timeout: 10000 }, async () => {
    await withHost(async (host, f) => {
      const pending = host.proposePlayerCompanyAction(proposal);
      const result = pending.then(value => ({ ok: true as const, value }), (error: unknown) => ({ ok: false as const, error }));
      const engine = f.workers[0], sent = engine.sent.at(-1)!;
      if (replacement !== "dispose") f.baseline(replacement === "same-generation" ? 1 : 2);
      engine.emit(settlement === "success"
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
