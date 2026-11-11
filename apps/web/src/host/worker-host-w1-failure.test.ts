import assert from "node:assert/strict";
import test from "node:test";
import { drain, fixture } from "./worker-host-w1.test-support.ts";

for (const phase of ["initializing", "paused", "running", "stopped"] as const) {
  test(`W1 ${phase} spawn 构造失败立即交必需 sink 且保持 engine 可用`, { timeout: 10000 }, async () => {
    const f = fixture({ childConstructError: new Error("child 构造失败", { cause: { marker: "原始 cause" } }) });
    try {
      f.launch(); await drain();
      if (phase !== "initializing") {
        f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
        assert.equal(f.outcome.status, "fulfilled");
        if (f.outcome.status !== "fulfilled") throw new Error("fixture 初始化失败");
        if (phase === "running" || phase === "stopped") {
          const starting = f.outcome.host.start(() => true);
          await drain();
          const request = f.workers[0].sent.at(-1)!;
          f.workers[0].emit({ type: "started", generation: 1, requestId: request.requestId }); await starting;
        }
        if (phase === "stopped") {
          const stopping = f.outcome.host.stop();
          await drain();
          const request = f.workers[0].sent.at(-1)!;
          f.workers[0].emit({ type: "stopped", generation: 1, requestId: request.requestId }); await stopping;
        }
      }
      f.spawn(); await drain();
      assert.equal(f.failures.length, 1, "不能等待 start 或下次 UI 操作才显示 spawnErr");
      assert.equal(f.failures[0].code, "WORKER_CONSTRUCT");
      assert.deepEqual((f.failures[0].cause as { cause: unknown }).cause, { marker: "原始 cause" });
      assert.deepEqual(f.failures[0].context, { workerId: 1, slot: 0, gen: 0 });
      assert.equal(f.workers[0].terminations, 0, "正常 spawnErr 由 Rust Result 消费，不升级为 engine fatal");
    } finally { await f.cleanup(); }
  });
}

test("W1 child bootstrap 的结构化失败保留原 HostFailure 而非全改 fatal", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.launch(); await drain(); f.spawn(); await drain();
    const child = f.workers.find((worker) => worker.sent.some((message) => message.type === "facadeChildBootstrap"));
    const original = { code: "WASM_IMPORT_UNRESOLVED", where: "fixture.instantiate", message: "缺少 import", cause: { import: "env.foo" }, context: { phase: "instantiate" }, recoverable: false, recoveryActions: ["反馈详情"] };
    if (child?.controlPort !== undefined) {
      const boot = child.sent[0];
      child.controlPort.postMessage({ type: "facadeBootstrapFailure", phase: "instantiate", workerId: boot.workerId, slot: boot.slot, gen: boot.gen, failure: original });
    }
    await drain();
    assert.deepEqual(f.failures, [original]);
    assert.equal(f.workers[0].terminations, 0);
    assert.equal(child?.terminations, 1);
  } finally { await f.cleanup(); }
});

test("W1 sink 抛错由 supervisor 统一 fatal 清理初始化在途资源", { timeout: 10000 }, async () => {
  const f = fixture({ childConstructError: new Error("child 失败"), spawnSink: () => { throw new Error("UI sink 失败"); } });
  try {
    f.launch(); await drain(); f.spawn(); await drain();
    f.clock.advance(2000); await drain();
    assert.equal(f.outcome.status, "rejected");
    if (f.outcome.status === "rejected") assert.match(String(f.outcome.error), /UI sink 失败|FACADE_SPAWN_REPORTER_ERROR/);
    assert.equal(f.workers[0].terminations, 1);
    assert.equal(f.clock.tasks.size, 0);
  } finally { await f.cleanup(); }
});

test("W1 native engine ErrorEvent 保留 WASM_WORKER_SCRIPT 壳与原 cause", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.launch(); await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
    assert.equal(f.outcome.status, "fulfilled");
    if (f.outcome.status !== "fulfilled") throw new Error("fixture 初始化失败");
    const failures: unknown[] = [];
    const starting = f.outcome.host.start(() => true, (failure) => { failures.push(failure); });
    await drain();
    const request = f.workers[0].sent.at(-1)!;
    f.workers[0].emit({ type: "started", generation: 1, requestId: request.requestId }); await starting;
    const original = new Error("原始脚本异常", { cause: { marker: "保真" } });
    f.workers[0].error(original, "浏览器脚本异常"); await drain();
    f.clock.advance(2000); await drain();
    assert.equal(failures.length, 1);
    const failure = failures[0] as { code: string; cause?: { message?: string; cause?: unknown } };
    assert.equal(failure.code, "WASM_WORKER_SCRIPT");
    assert.equal(failure.cause?.message, "原始脚本异常");
    assert.deepEqual(failure.cause?.cause, { marker: "保真" });
    assert.ok(f.workers.every((worker) => worker.terminations === 1));
  } finally { await f.cleanup(); }
});
