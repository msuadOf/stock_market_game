import assert from "node:assert/strict";
import test from "node:test";
import { drain, fixture, setup } from "./worker-host-w1.test-support.ts";
import { createEngineSupervisor } from "./engine-supervisor.ts";

for (const invalid of [undefined, {}, { onSpawnFailure: null }, { onSpawnFailure: 7 }]) {
  test(`W1 在分配资源前拒绝无效 onSpawnFailure ${String(invalid)}`, { timeout: 10000 }, async () => {
    const f = fixture();
    try {
      // 显式传 undefined 用来检查运行时调用者；fixture 的缺省 sink 不参与本用例。
      f.launch(invalid);
      await drain();
      if (f.outcome.status === "pending") { f.baseline(); await drain(); }
      assert.equal(f.outcome.status, "rejected", "缺少必需 sink 必须显式拒绝");
      assert.equal(f.workers.length, 0, "拒绝必须早于 new Worker");
      assert.equal(f.channels.length, 0, "拒绝必须早于 MessageChannel");
    } finally { await f.cleanup(); }
  });
}

test("W1 init 首次传 setup 和唯一控制端，ACK 后 create 只传 seed", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.launch(); await drain();
    const init = f.workers[0].sent[0];
    assert.equal(init.type, "init");
    assert.equal(init.setup, setup);
    assert.ok(init.controlPort instanceof MessagePort);
    assert.deepEqual(f.workers[0].transfers[0], [init.controlPort]);
    assert.equal(f.configured, 1, "ready 必须依赖真实 MessageChannel configured ACK");
    assert.deepEqual(f.workers[0].sent.find((message) => message.type === "create"), { type: "create", seed: 42n });
    assert.equal(f.workers[0].sent.filter((message) => Object.hasOwn(message, "controlPort")).length, 1);
  } finally { await f.cleanup(); }
});

test("W1 main 初始化总 deadline 在首次 bootstrap post 前已登记", { timeout: 10000 }, async () => {
  let timerExistsAtPost = false;
  const f = fixture({ beforeInitPost: (clock) => { timerExistsAtPost = [...clock.tasks.values()].some((task) => task.at === 10000); } });
  try { f.launch(); await drain(); assert.equal(timerExistsAtPost, true); }
  finally { await f.cleanup(); }
});

for (const kind of ["post", "channel"] as const) {
  test(`W1 ${kind} 失败立即清理刚构造的 engine`, { timeout: 10000 }, async () => {
    const f = fixture(kind === "post" ? { initError: new Error("bootstrap post 失败") } : { channelError: true });
    try {
      f.launch(); await drain();
      if (f.outcome.status === "pending") { f.baseline(); await drain(); }
      assert.equal(f.outcome.status, "rejected");
      assert.equal(f.workers[0].terminations, 1, "不得留下尚未 resolve 的 engine");
      assert.equal(f.clock.tasks.size, 0, "失败必须取消初始化 timer");
      assert.equal(f.workers[0].listenerCount(), 0, "失败必须撤除本人监听器");
    } finally { await f.cleanup(); }
  });
}

test("W1 dispose 取消初始化完成后所有本人监听器和 timer", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.launch(); await drain(); f.ingress(); f.baseline(); await drain();
    f.ack(f.workers[1], 1); await drain();
    assert.equal(f.outcome.status, "fulfilled");
    if (f.outcome.status !== "fulfilled") throw new Error("fixture 未完成初始化");
    await f.outcome.host.dispose();
    assert.ok(f.workers.every((worker) => worker.terminations === 1));
    assert.ok(f.workers.every((worker) => worker.listenerCount() === 0), "dispose 不能留下 host message/error listener");
    assert.equal(f.clock.tasks.size, 0);
    assert.ok(f.channels.every((channel) => (f.portCloses.get(channel.port1) ?? 0) > 0));
  } finally { await f.cleanup(); }
});


test("W1 fixture 控制项：真实 MessageChannel 完成 substrate/ACK 往返才发 ready", { timeout: 10000 }, async () => {
  const f = fixture();
  let supervisor: ReturnType<typeof createEngineSupervisor> | undefined;
  try {
    const engine = new Worker(new URL("./wasm-worker.ts", import.meta.url));
    const ready: unknown[] = [];
    engine.addEventListener("message", (event) => { ready.push(event.data); });
    supervisor = createEngineSupervisor({ engine, createChildWorker: () => { throw new Error("控制项禁止 child"); }, onFatalFailure: (failure) => { throw new Error(failure.message); }, onSpawnFailure: (failure) => { throw new Error(failure.message); } });
    assert.equal(f.configured, 0);
    supervisor.bootstrapEngine({ type: "init", setup });
    await drain();
    assert.equal(f.configured, 1); assert.deepEqual(ready, [{ type: "ready" }]);
    assert.doesNotThrow(() => supervisor!.assertConfigured());
    assert.ok(f.configuredSubstrate);
    assert.notEqual(f.configuredSubstrate.memory, f.substrate.memory, "真实 clone 为 main 产生独立 wrapper");
    new Int32Array(f.configuredSubstrate.memory.buffer)[30] = 45;
    assert.equal(f.words[30], 45, "不同 wrapper 仍共享同一 memory 数据");
  } finally { supervisor?.dispose(); await f.cleanup(); }
});

test("W1 dispose 收回仍在 bootstrap 的 child 和在途 query", { timeout: 10000 }, async () => {
  const f = fixture();
  try {
    f.launch(); await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
    const result = f.readOutcome(); assert.equal(result.status, "fulfilled");
    if (result.status !== "fulfilled") throw new Error("fixture 初始化失败");
    const query = result.host.civilDate!();
    const rejected = assert.rejects(query, /销毁|取消|关闭/);
    f.spawn(); await drain();
    assert.equal(f.workers.length, 3, "fixture 必须实际创建 engine/ingress/child");
    await assert.rejects(result.host.dispose(), (error: unknown) => {
      assert.ok(error instanceof AggregateError);
      assert.match(String((error.cause as { release: unknown }).release), /quiescence|停止 grow/);
      return true;
    }); await rejected;
    assert.ok(f.workers.every((worker) => worker.terminations === 1));
    assert.ok(f.workers.every((worker) => worker.listenerCount() === 0));
    assert.equal(f.clock.tasks.size, 0);
  } finally { await f.cleanup(/最终释放事实未知/); }
});
