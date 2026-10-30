import assert from "node:assert/strict";
import test from "node:test";
import { createWorkerHost } from "./worker-host.ts";
import { setup } from "./worker-host-w1.test-support.ts";
import { createEngineControlClient } from "./engine-control-client.ts";

test("WorkerHost 首次构造主线程 Worker 时只创建 controlWorker", { timeout: 10000 }, async () => {
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  const constructions: { readonly url: string; readonly options: unknown }[] = [];
  const constructionFailure = new Error("controlWorker 构造故障");
  class FailingWorker {
    constructor(url: URL, options?: WorkerOptions) {
      constructions.push({ url: url.href, options });
      throw constructionFailure;
    }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, writable: true, value: FailingWorker });
  try {
    await assert.rejects(createWorkerHost(setup, 42n, { onSpawnFailure: () => {} }));
    assert.equal(constructions.length, 1, "构造失败前只能尝试创建一个 Worker");
    assert.ok(constructions[0].url.endsWith("/engine-control-worker.ts"), constructions[0].url);
    assert.equal((constructions[0].options as WorkerOptions | undefined)?.type, "module");
  } finally {
    if (original === undefined) Reflect.deleteProperty(globalThis, "Worker");
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("有效 owner failure 缺 terminal 时在原 2 秒期限拒绝并撤下本地主端引用", { timeout: 10000 }, async (context) => {
  context.mock.timers.enable({ apis: ["setTimeout"] });
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class SilentOwner extends EventTarget {
    static current: SilentOwner;
    readonly sent: Record<string, unknown>[] = [];
    terminations = 0;
    constructor() { super(); SilentOwner.current = this; }
    postMessage(message: Record<string, unknown>) { this.sent.push(message); }
    terminate() { this.terminations++; }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: SilentOwner });
  let client: ReturnType<typeof createEngineControlClient> | undefined;
  try {
    const failures: Error[] = [];
    client = createEngineControlClient({ runtimeOverride: false, onFailure: (error) => failures.push(error), onSpawnFailure() {} });
    client.start();
    const owner = SilentOwner.current;
    const rejected = assert.rejects(client.release, /缺少真实 terminal.*unknown/);
    let released = false;
    void client.release.then(() => { released = true; }, () => { released = true; });
    owner.dispatchEvent(new MessageEvent("message", { data: { type: "failure", instanceId: client.instanceId,
      failure: { code: "OWNER_FAILURE", where: "fixture.owner", message: "原 owner 故障" } } }));
    await assert.rejects(client.configured, /OWNER_FAILURE/);
    context.mock.timers.tick(1999);
    await Promise.resolve();
    assert.equal(released, false);
    assert.equal(owner.terminations, 0);
    context.mock.timers.tick(1);
    await rejected;
    assert.equal(owner.terminations, 1, "同一 2 秒届满必须撤下 Worker，不再追加 2 秒宽限");
    assert.throws(() => client!.enginePort.postMessage({ type: "create" }), /已关闭/);
    assert.equal(owner.sent.at(-1)?.type, "abort");
    assert.match(failures[0].message, /OWNER_FAILURE/);
  } finally {
    for (const message of SilentOwner.current?.sent ?? []) if (message.dataPort instanceof MessagePort) message.dataPort.close();
    client?.abortShutdown(new Error("fixture 收尾"));
    if (original === undefined) Reflect.deleteProperty(globalThis, "Worker");
    else Object.defineProperty(globalThis, "Worker", original);
  }
});
