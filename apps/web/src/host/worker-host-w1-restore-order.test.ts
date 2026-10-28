import { catalogWorkerFixture } from "./catalog-test-fixture.ts";
import assert from "node:assert/strict";
import test from "node:test";
import { currentSaveFixture } from "../save/current-save-fixture.ts";
import { snapshot } from "./protocol-test-fixtures.ts";
import { drain, fixture } from "./worker-host-w1.test-support.ts";

const cancel = { Cancel: { code: "000812", id: 7 } } as const;

test("W1 真实 load 在途时 ingress 新桥先于 restored/baseline 合法，提交使用新代 token", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.launch(); await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
    const initialized = f.readOutcome(); assert.equal(initialized.status, "fulfilled");
    if (initialized.status !== "fulfilled") throw new Error("fixture 初始化失败");
    const host = initialized.host, engine = f.workers[0], intake = f.workers[1];
    let loaded = false;
    const loading = host.load(currentSaveFixture()).then(() => { loaded = true; });
    // 立即接住真实 load 的失败，避免断言先失败时产生未处理 rejection。
    const loadResult = loading.then(() => ({ ok: true as const }), (error: unknown) => ({ ok: false as const, error }));
    const restore = engine.sent.at(-1)!;
    assert.equal(restore.type, "restore"); assert.equal(restore.generation, 1);
    assert.equal(loaded, false, "必须先证明 load 已在途");

    // wasm-worker.restore 的真实发送顺序：postIngressBridge → restored → baseline。
    f.ingress(2, 202);
    assert.equal(engine.terminations, 0, "合法在途 restore 的先行 ingress2 不能触发 fatal");
    const binding = intake.sent.at(-1)!;
    assert.equal(binding.type, "bindIngress"); assert.equal(binding.generation, 2); assert.equal(binding.token, 202);
    engine.emit({ ...catalogWorkerFixture(snapshot(2, 2)), type: "restored", requestId: restore.requestId, generation: 1, nextGeneration: 2, snapshot: snapshot(2, 2), civilDate: "2030-01-02" });
    f.baseline(2); await drain();
    f.ack(intake, 2); await drain();
    assert.deepEqual(await loadResult, { ok: true }); assert.equal(host.tick(), 2);

    let submitted = false;
    const submitting = host.submitIntent(cancel).then(() => { submitted = true; });
    const submitResult = submitting.then(() => ({ ok: true as const }), (error: unknown) => ({ ok: false as const, error }));
    await drain();
    const enqueue = intake.sent.at(-1)!;
    assert.equal(enqueue.type, "enqueue"); assert.equal(enqueue.generation, 2);
    assert.equal(submitted, false, "只有新桥 enqueue ACK 才能确认提交");
    assert.equal(engine.sent.some((message) => message.type === "enqueue"), false);
    intake.emit({ type: "enqueued", requestId: enqueue.requestId, generation: 2 }); await drain();
    assert.deepEqual(await submitResult, { ok: true });
    assert.equal(engine.terminations, 0); assert.equal(intake.terminations, 0); assert.equal(f.workers.length, 2);
  } finally { await f.cleanup(); }
});

for (const staleType of ["enqueued", "operationError"] as const) {
  test(`W1 restore 新桥先行时旧请求 ${staleType} 不得误成功或污染新桥`, { timeout: 10000 }, async () => {
    const f = fixture();
    try {
      f.launch(); await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
      const initialized = f.readOutcome(); assert.equal(initialized.status, "fulfilled");
      if (initialized.status !== "fulfilled") throw new Error("fixture 初始化失败");
      const host = initialized.host, engine = f.workers[0], intake = f.workers[1];
      const oldSubmitting = host.submitIntent(cancel);
      let oldSucceeded = false;
      const oldResult = oldSubmitting.then(() => { oldSucceeded = true; return { ok: true as const }; }, (error: unknown) => ({ ok: false as const, error }));
      await drain();
      const oldEnqueue = intake.sent.at(-1)!; assert.equal(oldEnqueue.type, "enqueue"); assert.equal(oldEnqueue.generation, 1);
      const loading = host.load(currentSaveFixture());
      const loadResult = loading.then(() => ({ ok: true as const }), (error: unknown) => ({ ok: false as const, error }));
      const restore = engine.sent.at(-1)!; assert.equal(restore.type, "restore");
      f.ingress(2, 202);
      // 新桥已发布、restored 尚未返回：旧 ACK 不能趁 currentGeneration 仍是 1 而成功。
      intake.emit({ type: staleType, requestId: oldEnqueue.requestId, generation: 1, message: "旧桥迟到失败" });
      f.ack(intake, 1, staleType === "operationError" ? "旧 bind 迟到失败" : undefined);
      await drain();
      assert.equal(oldSucceeded, false);
      const old = await oldResult; assert.equal(old.ok, false);
      assert.equal(engine.terminations, 0); assert.equal(intake.terminations, 0);

      engine.emit({ ...catalogWorkerFixture(snapshot(2, 2)), type: "restored", requestId: restore.requestId, generation: 1, nextGeneration: 2, snapshot: snapshot(2, 2), civilDate: "2030-01-02" });
      f.baseline(2); await drain(); f.ack(intake, 2); await drain();
      assert.deepEqual(await loadResult, { ok: true });
      let newSucceeded = false;
      const newSubmitting = host.submitIntent(cancel);
      const newResult = newSubmitting.then(() => { newSucceeded = true; return { ok: true as const }; }, (error: unknown) => ({ ok: false as const, error }));
      await drain();
      const enqueue = intake.sent.at(-1)!; assert.equal(enqueue.type, "enqueue"); assert.equal(enqueue.generation, 2);
      intake.emit({ type: "enqueued", requestId: enqueue.requestId, generation: 1 }); await drain();
      assert.equal(newSucceeded, false, "即使 requestId 相同，旧 generation ACK 也不能确认新请求");
      intake.emit({ type: "enqueued", requestId: enqueue.requestId, generation: 2 }); await drain();
      assert.deepEqual(await newResult, { ok: true });
      assert.equal(engine.terminations, 0); assert.equal(intake.terminations, 0);
    } finally { await f.cleanup(); }
  });
}


for (const type of ["ready", "baseline"] as const) {
  test(`W1 configured ACK 前的全局 ${type} 必须拒绝初始化并清理`, { timeout: 10000 }, async () => {
    const f = fixture();
    try {
      f.launch();
      assert.equal(f.configured, 0, "不 drain：原生控制端 ACK 此时尚未返回");
      if (type === "ready") f.workers[0].emit({ type: "ready" });
      else f.baseline();
      await drain();
      assert.equal(f.workers[0].sent.some((message) => message.type === "create"), false, "早到消息不能先发 create 再补 configured");
      assert.equal(f.outcome.status, "rejected", "不能借未 configured 的 ready/baseline 开始会话");
      assert.equal(f.workers[0].terminations, 1);
      assert.equal(f.clock.tasks.size, 0);
    } finally { await f.cleanup(); }
  });
}
