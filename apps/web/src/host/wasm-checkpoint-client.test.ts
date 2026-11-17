import assert from "node:assert/strict";
import { test } from "node:test";
import type { FacadeSubstrate } from "./engine-supervisor.ts";
import { WasmCheckpointClient, type CheckpointBindings } from "./wasm-checkpoint-client.ts";

test("日终写盘失败保持 blocked，迟到较晚成功不得冒充耐久，关闭显式失败", async () => {
  class FakeWorker extends EventTarget {
    static instance: FakeWorker;
    constructor() { super(); FakeWorker.instance = this; }
    postMessage() { this.deliver({ type: "ready" }); }
    deliver(data: unknown) { this.dispatchEvent(new MessageEvent("message", { data })); }
  }
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: FakeWorker });
  const receipts: bigint[] = [];
  const warnings: string[] = [];
  const client = new WasmCheckpointClient();
  let latest = "10";
  const bindings: CheckpointBindings = {
    market_archive_attach() {}, market_archive_latest() { return latest; }, market_archive_tombstone() { return 1; },
    market_archive_receipt(_handle, generation, _epoch, seq, _failure) { assert.equal(generation, 2n); receipts.push(seq); return _failure === undefined ? "durable" : "failed"; },
    market_archive_failed() {}, market_archive_finish() {},
    market_archive_detach() { queueMicrotask(() => FakeWorker.instance.deliver({ type: "closed" })); },
  };
  const substrate = { module: {}, memory: {}, layout: {}, diagnostics: false } as unknown as FacadeSubstrate;
  try {
    await client.attach(1, bindings, 2, substrate, error => warnings.push(error.message));
    const inFlight = client.waitIdle(1, bindings);
    FakeWorker.instance.deliver({ type: "writeFailed", seq: "10", message: "磁盘暂不可用" });
    await assert.rejects(inFlight, /10.*磁盘暂不可用/);
    await assert.rejects(client.waitIdle(1, bindings), /10.*磁盘暂不可用/);
    assert.equal(warnings.length, 1);
    assert.match(warnings[0], /10.*磁盘暂不可用/);
    assert.deepEqual(receipts, [10n]);

    latest = "20";
    await assert.rejects(client.waitIdle(1, bindings), /10.*磁盘暂不可用/);
    FakeWorker.instance.deliver({ type: "stored", seq: "20" });
    await assert.rejects(client.waitIdle(1, bindings), /blocked/);
    assert.deepEqual(receipts, [10n]);
    await assert.rejects(async () => { await client.close(); }, /blocked/);
  } finally {
    if (original === undefined) Reflect.deleteProperty(globalThis, "Worker");
    else Object.defineProperty(globalThis, "Worker", original);
  }
});


test("Rust 拒绝 stored 收据时客户端不得发布假耐久", { timeout: 10000 }, async () => {
  class FakeWorker extends EventTarget {
    static instance: FakeWorker;
    constructor() { super(); FakeWorker.instance = this; }
    postMessage() { this.deliver({ type: "ready" }); }
    deliver(data: unknown) { this.dispatchEvent(new MessageEvent("message", { data })); }
  }
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: FakeWorker });
  const client = new WasmCheckpointClient();
  const bindings: CheckpointBindings = {
    market_archive_attach() {}, market_archive_latest() { return "10"; }, market_archive_tombstone() { return 1; },
    market_archive_receipt() { return "failed"; }, market_archive_failed() {}, market_archive_finish() {},
    market_archive_detach() { queueMicrotask(() => FakeWorker.instance.deliver({ type: "closed" })); },
  };
  try {
    await client.attach(1, bindings, 2, { module: {}, memory: {}, layout: {}, diagnostics: false } as unknown as FacadeSubstrate, () => {});
    const waiting = client.waitIdle(1, bindings);
    FakeWorker.instance.deliver({ type: "stored", seq: "10" });
    await assert.rejects(waiting, /未确认.*耐久/);
    await assert.rejects(client.waitIdle(1, bindings), /未确认.*耐久/);
    await assert.rejects(async () => { await client.close(); }, /未确认.*耐久/);
    await client.close(true);
  } finally {
    if (original === undefined) Reflect.deleteProperty(globalThis, "Worker");
    else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("关停预算为归档退出后的 Rust 清理预留时间", async () => {
  const { WASM_SHUTDOWN_BUDGET_MS, CHECKPOINT_EXIT_BUDGET_MS } = await import("./wasm-shutdown-budget.ts");
  assert.ok(CHECKPOINT_EXIT_BUDGET_MS < WASM_SHUTDOWN_BUDGET_MS);
  assert.ok(WASM_SHUTDOWN_BUDGET_MS < 10_000);
});

test("已确认退出的未耐久关闭释放连接但不报告成功", async () => {
  const { CheckpointCloseError } = await import("./wasm-checkpoint-client.ts");
  class FakeWorker extends EventTarget {
    static instance: FakeWorker;
    constructor() { super(); FakeWorker.instance = this; }
    postMessage() { this.dispatchEvent(new MessageEvent("message", { data: { type: "ready" } })); }
  }
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: FakeWorker });
  const finished: boolean[] = [];
  const bindings: CheckpointBindings = {
    market_archive_attach() {}, market_archive_latest() { return null; }, market_archive_tombstone() { return 1; },
    market_archive_receipt() { return "durable"; }, market_archive_failed() {},
    market_archive_detach() { queueMicrotask(() => FakeWorker.instance.dispatchEvent(new MessageEvent("message", { data: { type: "closed" } }))); },
    market_archive_finish(abandonedHandle, abandon) { assert.equal(abandonedHandle, 1); finished.push(abandon); if (!abandon) throw new Error("sealed 未耐久"); },
  };
  const client = new WasmCheckpointClient();
  try {
    await client.attach(1, bindings, 1, {} as FacadeSubstrate, () => {});
    await assert.rejects(async () => client.close(), error => error instanceof CheckpointCloseError && error.quiescence === "confirmed" && error.message.includes("sealed 未耐久"));
    assert.deepEqual(finished, [false, true]);
    assert.equal(client.close(true), undefined);
  } finally {
    if (original === undefined) Reflect.deleteProperty(globalThis, "Worker"); else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("退出预算耗尽保留旧连接且禁止重复挂接", async () => {
  const { CheckpointCloseError } = await import("./wasm-checkpoint-client.ts");
  class FakeWorker extends EventTarget { postMessage() { this.dispatchEvent(new MessageEvent("message", { data: { type: "ready" } })); } }
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: FakeWorker });
  let finished = false;
  const bindings: CheckpointBindings = {
    market_archive_attach() {}, market_archive_latest() { return null; }, market_archive_tombstone() { return 1; },
    market_archive_receipt() { return "durable"; }, market_archive_failed() {}, market_archive_detach() {}, market_archive_finish() { finished = true; },
  };
  const client = new WasmCheckpointClient();
  try {
    await client.attach(1, bindings, 1, {} as FacadeSubstrate, () => {});
    await assert.rejects(async () => client.close(false, 0), error => error instanceof CheckpointCloseError && error.quiescence === "unknown");
    assert.equal(finished, false);
    assert.throws(() => client.attach(1, bindings, 1, {} as FacadeSubstrate, () => {}), /旧归档副实例/);
  } finally {
    if (original === undefined) Reflect.deleteProperty(globalThis, "Worker"); else Object.defineProperty(globalThis, "Worker", original);
  }
});

test("真实归档 Worker 强退注入在非验证环境不可用", () => {
  assert.throws(() => new WasmCheckpointClient().forceUnknownExitForVerification(), /验证|归档连接/);
});
