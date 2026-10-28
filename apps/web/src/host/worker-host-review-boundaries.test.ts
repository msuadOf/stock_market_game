import assert from "node:assert/strict";
import { test } from "node:test";
import { fixture, drain } from "./worker-host-w1.test-support.ts";

test("review 真实create先ingress再created/baseline，首桥不能因currentGeneration尚为0拒绝", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.launch();
    const engine = f.workers[0], original = engine.onPost;
    engine.onPost = (value, transfer) => {
      if (value.type === "create") queueMicrotask(() => f.ingress(1, 7));
      original?.(value, transfer);
      if (value.type === "create") queueMicrotask(() => f.baseline(1));
    };
    await drain();
    assert.equal(engine.terminations, 0, "已批wasm-worker.create在created前发送首桥是合法顺序");
    assert.equal(f.workers.length, 2);
    f.ack(f.workers[1], 1); await drain();
    assert.equal(f.outcome.status, "fulfilled");
  } finally { await f.cleanup(); }
});

for (const transition of ["dispose", "generation"]) {
  test(`review civilDate终端读取必须在 ${transition} 前完成或显式拒绝`, { timeout: 10000 }, async () => {
    const f = fixture();
    try {
      f.launch(); await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
      const ready = f.readOutcome(); assert.equal(ready.status, "fulfilled");
      if (ready.status !== "fulfilled") throw new Error("fixture 初始化失败");
      let changed = false;
      const terminalReads: boolean[] = [];
      const pending = ready.host.civilDate!().then(value => ({ status: "fulfilled" as const, value }), error => ({ status: "rejected" as const, error }));
      const request = f.workers[0].sent.at(-1)!;
      // getter仅观察测试中终端date读取的发生时点，不参与guard或伪造业务结果。
      const response = { type: "civilDate", generation: 1, requestId: request.requestId };
      Object.defineProperty(response, "date", { enumerable: true, get() { terminalReads.push(changed); return "2030-01-02"; } });
      f.workers[0].emit(response);
      queueMicrotask(() => { changed = true; if (transition === "dispose") void ready.host.dispose(); else f.baseline(2); });
      const outcome = await pending;
      if (outcome.status === "fulfilled") {
        assert.ok(terminalReads.length > 0);
        assert.equal(terminalReads.some(Boolean), false, "guard通过后不能跨await，在已作废host上读取并交付旧响应");
        assert.equal(outcome.value, "2030-01-02");
      } else assert.match(String(outcome.error), /过期|销毁/);
    } finally { await f.cleanup(); }
  });
}

test("review 直接Worker failure后的fatal callback抛错必须显式报告且已清本人资源", { timeout: 10000 }, async () => {
  const f = fixture();
  const oldQueue = globalThis.queueMicrotask, oldReporter = Object.getOwnPropertyDescriptor(globalThis, "reportError");
  const queued: (() => void)[] = [], reported: unknown[] = [];
  try {
    f.launch(); await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
    const ready = f.readOutcome(); assert.equal(ready.status, "fulfilled");
    if (ready.status !== "fulfilled") throw new Error("fixture 初始化失败");
    const original = new Error("REVIEW_FATAL_CALLBACK_CAUSE");
    let invoked = 0;
    const starting = ready.host.start(() => true, () => { invoked++; throw original; });
    const request = f.workers[0].sent.at(-1)!;
    f.workers[0].emit({ type: "started", generation: 1, requestId: request.requestId }); await starting;
    globalThis.queueMicrotask = callback => { queued.push(callback); };
    Object.defineProperty(globalThis, "reportError", { configurable: true, value: (error: unknown) => { reported.push(error); } });
    try { f.workers[0].emit({ type: "failure", code: "ENGINE_FATAL", where: "review", message: "原始engine fatal" }); }
    catch (error) { reported.push(error); }
    assert.equal(invoked, 1);
    assert.ok(f.workers.every(worker => worker.terminations === 1));
    assert.equal(f.clock.tasks.size, 0);
    assert.ok(reported.length + queued.length > 0, "onMessage catch不能因disposed=true静默吞掉fatal callback原异常");
  } finally {
    globalThis.queueMicrotask = oldQueue;
    if (oldReporter === undefined) Reflect.deleteProperty(globalThis, "reportError"); else Object.defineProperty(globalThis, "reportError", oldReporter);
    await f.cleanup();
  }
});
