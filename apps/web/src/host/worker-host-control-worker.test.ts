import assert from "node:assert/strict";
import test from "node:test";
import { createWorkerHost } from "./worker-host.ts";
import { setup } from "./worker-host-w1.test-support.ts";

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
