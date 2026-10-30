import assert from "node:assert/strict";
import test from "node:test";
import { drain, fixture } from "./worker-host-w1.test-support.ts";
import { JoinState, encodeJoin, joinWordOffset } from "./facade-slot-protocol.ts";

for (const order of ["reply-first", "exit-first"] as const) {
  test(`正常 shutdown ${order} 必须等 Rust reply 和真实 childExit 齐全才 terminate/release`, { timeout: 10000 }, async () => {
    const f = fixture();
    try {
      f.launch(); await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
      const outcome = f.readOutcome(); if (outcome.status !== "fulfilled") throw new Error("fixture 未就绪");
      f.spawn(); await drain(); const child = f.workers[2];
      const init = child.sent[0];
      child.controlPort!.postMessage({ type: "facadeChildReady", workerId: init.workerId, slot: 0, gen: 0 }); await drain();
      const engine = f.workers[0], original = engine.onPost;
      engine.onPost = (message, transfer) => { if (message.type !== "shutdown") original?.(message, transfer); };
      let settled = false;
      const closing = outcome.host.dispose(); void closing.then(() => { settled = true; });
      await drain();
      const request = engine.sent.findLast((message) => message.type === "shutdown");
      assert.ok(request, "DISPOSE_MUST_REQUEST_REAL_SHUTDOWN");
      const reply = () => {
        engine.emit({ type: "shutdownCompleted", requestId: request.requestId, generation: 1,
          report: { kind: "success", handle: 7, runtime: { ownedWorkers: 1, joinedWorkers: 1, workers: [{ worker: "group-0", result: { kind: "ok" } }], groupFinals: 1, deskFinals: 0 } } });
        engine.emit({ type: "engineLifecycle", instanceId: engine.instanceId, state: "quiescent" });
      };
      const exit = () => {
        Atomics.store(f.words, joinWordOffset(0, f.layout.capacity), encodeJoin(0, JoinState.Consumed, 29) | 0);
        child.controlPort!.postMessage({ type: "facadeWorkerExit", workerId: init.workerId, slot: 0, gen: 0 });
      };
      if (order === "reply-first") reply(); else exit();
      await drain(); assert.equal(settled, false); assert.equal(engine.terminations, 0);
      if (order === "reply-first") exit(); else reply();
      await closing;
      assert.equal(settled, true); assert.equal(engine.terminations, 1); assert.equal(f.workers[1].terminations, 1);
      assert.equal(outcome.host.dispose(), closing, "重复 dispose 复用同一 Promise");
    } finally { await f.cleanup(); }
  });
}

for (const scenario of ["bad-options", "constructor", "allocated-init"] as const) {
  test(`release 在首次资源前登记，${scenario} 保持确切分配事实`, { timeout: 10000 }, async () => {
    const f = fixture(scenario === "constructor" ? { engineConstructError: new Error("constructor rejected") } : scenario === "allocated-init" ? { initAfterMemoryFailure: "wait rejected after allocation" } : {});
    let release: Promise<unknown> | undefined;
    let registrations = 0;
    try {
      const creating = f.launch({ onSpawnFailure: scenario === "bad-options" ? null : () => {}, memoryAdmissionTicket: {
        registerRelease(value: Promise<unknown>) { assert.equal(f.workers.length, 0); registrations++; release = value; },
      } });
      await assert.rejects(creating);
      assert.equal(registrations, 1); assert.ok(release);
      assert.deepEqual(await release, scenario === "allocated-init" ? { kind: "released", finalBytes: 131072, growthStopped: true, referencesDropped: true } : { kind: "not-allocated" });
      if (scenario === "allocated-init") assert.equal(f.workers[0].terminations, 1);
    } finally { await f.cleanup(); }
  });
}

test("Rust shutdown 失败保留完整 cause，真实 quiescence 仍可结账但 dispose 不得成功", { timeout: 10000 }, async () => {
  const f = fixture();
  const observations: unknown[] = [];
  let release!: Promise<unknown>;
  try {
    f.launch({ onSpawnFailure() {}, lifecycleObserver: (event: unknown) => { observations.push(event); }, memoryAdmissionTicket: { registerRelease(value: Promise<unknown>) { release = value; } } });
    await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
    const outcome = f.readOutcome(); if (outcome.status !== "fulfilled") throw new Error("fixture 未就绪");
    f.workers[0].onPost = (request) => {
      if (request.type === "shutdown") { f.workers[0].emit({ type: "shutdownCompleted", requestId: request.requestId, generation: request.generation, report: { kind: "failure", handle: 7,
        primary: { stage: "final", worker: null, detail: "缺少 Final" }, secondary: [{ stage: "cleanup", worker: null, detail: "次要故障" }], completion: { ownedWorkers: 0, joinedWorkers: 0, workers: [], groupFinals: 0, deskFinals: 0 } } });
        f.workers[0].emit({ type: "engineLifecycle", instanceId: f.workers[0].instanceId, state: "quiescent" }); }
    };
    await assert.rejects(outcome.host.dispose(), (error: unknown) => {
      assert.equal((error as Error).cause !== null, true);
      const cause = (error as { cause: { primary: { detail: string }; secondary: { detail: string }[]; completion: { joinedWorkers: number } } }).cause;
      assert.equal(cause.primary.detail, "缺少 Final"); assert.equal(cause.secondary[0].detail, "次要故障"); assert.equal(cause.completion.joinedWorkers, 0);
      return true;
    });
    assert.deepEqual(await release, { kind: "released", finalBytes: 65536, growthStopped: true, referencesDropped: true });
    assert.ok(observations.some((item) => (item as { type: string }).type === "finalRelease"));
  } finally { await f.cleanup(/Rust shutdown 失败/); }
});

for (const joined of [true, false]) {
  test(`fatal cleanup 的 typed shutdown ${joined ? "完整 join 可真实释放" : "部分 join 保持 unknown"}且保留原 HostFailure`, { timeout: 10000 }, async () => {
    const f = fixture();
    const failure = { code: "ORIGINAL_FATAL", where: "fixture.fatal", message: "原用户故障", cause: { marker: "原 cause" } };
    const shutdown = { kind: "failure", handle: 7, primary: { stage: "final", worker: null, detail: "typed shutdown 故障" }, secondary: [],
      completion: { ownedWorkers: joined ? 0 : 1, joinedWorkers: 0, workers: [], groupFinals: 0, deskFinals: 0 } };
    const observations: unknown[] = [], failures: unknown[] = [];
    let release!: Promise<unknown>;
    try {
      f.launch({ onSpawnFailure() {}, lifecycleObserver: (event: unknown) => observations.push(event), memoryAdmissionTicket: { registerRelease(value: Promise<unknown>) { release = value; } } });
      const released = joined ? release : assert.rejects(release, /unknown|未知/);
      await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
      const outcome = f.readOutcome(); if (outcome.status !== "fulfilled") throw new Error("fixture 未就绪");
      const starting = outcome.host.start(() => true, (value) => failures.push(value));
      await drain();
      const engine = f.workers[0], request = engine.sent.at(-1)!;
      engine.emit({ type: "started", requestId: request.requestId, generation: request.generation });
      await starting;
      engine.onPost = (message) => {
        if (message.type === "shutdown") {
          engine.emit({ type: "shutdownCompleted", requestId: message.requestId, generation: message.generation, report: shutdown });
          engine.emit({ type: "engineLifecycle", instanceId: engine.instanceId, state: "quiescent" });
        }
      };
      engine.emit({ type: "failure", generation: 1, ...failure });
      await drain();
      const receipt = await released;
      if (joined) assert.deepEqual(receipt, { kind: "released", finalBytes: 65536, growthStopped: true, referencesDropped: true });
      assert.deepEqual(failures, [failure], "cleanup shutdown 结果不得替换原 HostFailure");
      assert.ok(observations.some((event) => {
        if (event === null || typeof event !== "object" || (event as { type?: unknown }).type !== "rustShutdown") return false;
        assert.deepEqual((event as { report: unknown }).report, shutdown);
        return true;
      }), "保留完整 typed shutdown 诊断报告");
      assert.ok(f.workers.every((worker) => worker.terminations === 1));
    } finally { await f.cleanup(); }
  });
}

test("shutdown 超时强制 discard 不冒成功，release 未知且新业务被拒绝", { timeout: 10000 }, async () => {
  const f = fixture();
  let release!: Promise<unknown>;
  try {
    f.launch({ onSpawnFailure() {}, memoryAdmissionTicket: { registerRelease(value: Promise<unknown>) { release = value; } } });
    await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
    const outcome = f.readOutcome(); if (outcome.status !== "fulfilled") throw new Error("fixture 未就绪");
    f.workers[0].onPost = () => {};
    const closing = outcome.host.dispose();
    const businessFailure = assert.rejects(closing, /shutdown|未知|超时/);
    const releaseFailure = assert.rejects(release, /quiescence|停止 grow/);
    await assert.rejects(outcome.host.civilDate!(), /关闭/);
    await assert.rejects(outcome.host.load({}), /关闭/);
    f.clock.advance(2001); await drain();
    await businessFailure; await releaseFailure;
    assert.ok(f.workers.every((worker) => worker.terminations === 1));
  } finally { await f.cleanup(/shutdown|未知|超时/); }
});

test("已登记 release 后 initialArchive getter 异常保留 identity 并结 not-allocated", { timeout: 10000 }, async () => {
  const f = fixture();
  const original = new Error("initialArchive getter failed");
  let release: Promise<unknown> | undefined;
  try {
    const options = {
      onSpawnFailure() {},
      memoryAdmissionTicket: { registerRelease(value: Promise<unknown>) { assert.equal(f.workers.length, 0); release = value; } },
      get initialArchive() { throw original; },
    };
    await assert.rejects(f.launch(options), error => error === original);
    assert.ok(release);
    let settled = false; let receipt: unknown;
    void release.then(value => { settled = true; receipt = value; });
    await Promise.resolve();
    assert.equal(settled, true, "ticket 必须在确切未分配时结账");
    assert.deepEqual(receipt, { kind: "not-allocated" });
    assert.equal(f.workers.length, 0);
  } finally { await f.cleanup(); }
});


test("closing fatal 的完整 HostFailure 进入 dispose 主错，release unknown 仍为次错", { timeout: 10000 }, async () => {
  const f = fixture();
  const failure = { code: "TRAP", where: "fixture.native", message: "native trapped", cause: { detail: "TRAP_NATIVE_SENTINEL", primary: { detail: "Rust primary" }, secondary: [{ detail: "Rust secondary" }] }, context: { stage: "SHUTDOWN_STAGE_SENTINEL" }, recoverable: false, recoveryActions: ["复制错误详情反馈"] };
  let release!: Promise<unknown>;
  try {
    f.launch({ onSpawnFailure() {}, memoryAdmissionTicket: { registerRelease(value: Promise<unknown>) { release = value; } } });
    const released = assert.rejects(release, /quiescence|停止 grow/);
    await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
    const outcome = f.readOutcome(); if (outcome.status !== "fulfilled") throw new Error("fixture 未就绪");
    f.workers[0].onPost = request => { if (request.type === "shutdown") f.workers[0].emit(structuredClone({ type: "failure", generation: 1, ...failure })); };
    await assert.rejects(outcome.host.dispose(), (error: unknown) => {
      assert.ok(error instanceof AggregateError);
      const cause = error.cause as { primary: Error; release: Error };
      assert.deepEqual(cause.primary.cause, failure);
      assert.ok(cause.release instanceof Error);
      return true;
    });
    await released;
  } finally { await f.cleanup(/shutdown|未知|失败/); }
});

test("initialArchive 只读取一次快照，后续 getter 不能制造拓扑分叉", { timeout: 10000 }, async () => {
  const f = fixture();
  let reads = 0;
  try {
    f.launch({ onSpawnFailure() {}, get initialArchive() { if (++reads > 1) throw new Error("不能重复读取 initialArchive"); return undefined; } });
    await drain();
    assert.equal(reads, 1);
    assert.equal(f.workers.length, 1);
    f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
    assert.equal(f.readOutcome().status, "fulfilled");
  } finally { await f.cleanup(); }
});


test("Rust reply 后等待 child exit 期间的 fatal 仍保完整 HostFailure", { timeout: 10000 }, async () => {
  const f = fixture();
  const failure = { code: "AFTER_REPLY_TRAP", where: "fixture.child", message: "child failed after reply", cause: { primary: "PRIMARY_AFTER_REPLY", secondary: ["SECONDARY_AFTER_REPLY"] }, context: { stage: "WAIT_CHILD_EXIT" } };
  let release!: Promise<unknown>;
  try {
    f.launch({ onSpawnFailure() {}, memoryAdmissionTicket: { registerRelease(value: Promise<unknown>) { release = value; } } });
    const released = assert.rejects(release, /quiescence|停止 grow/);
    await drain(); f.ingress(); f.baseline(); await drain(); f.ack(f.workers[1], 1); await drain();
    const outcome = f.readOutcome(); if (outcome.status !== "fulfilled") throw new Error("fixture 未就绪");
    f.spawn(); await drain(); const child = f.workers[2], init = child.sent[0];
    child.controlPort!.postMessage({ type: "facadeChildReady", workerId: init.workerId, slot: 0, gen: 0 }); await drain();
    const closing = outcome.host.dispose();
    const rejected = assert.rejects(closing, (error: unknown) => {
      assert.ok(error instanceof AggregateError);
      const releaseCause = error.cause as { primary: Error };
      const observed = releaseCause.primary.cause as { shutdownReport: unknown; transport: Error };
      assert.deepEqual(observed.transport.cause, failure);
      assert.deepEqual(observed.shutdownReport, { kind: "success", handle: 7, runtime: null });
      return true;
    });
    await drain();
    f.workers[0].emit(structuredClone({ type: "failure", generation: 1, ...failure }));
    await rejected; await released;
  } finally { await f.cleanup(/shutdown|未知|失败/); }
});
