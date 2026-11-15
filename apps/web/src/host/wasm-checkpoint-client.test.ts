import assert from "node:assert/strict";
import { test } from "node:test";
import type { FacadeSubstrate } from "./engine-supervisor.ts";
import { WasmCheckpointClient, type CheckpointBindings } from "./wasm-checkpoint-client.ts";

test("一份日终写盘失败释放 FIFO 队头，却不被当成已耐久，也不阻止下一天落盘", async () => {
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
    market_archive_receipt(_handle, generation, seq) { assert.equal(generation, 2n); receipts.push(seq); },
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
    const following = client.waitIdle(1, bindings);
    FakeWorker.instance.deliver({ type: "stored", seq: "20" });
    await following;
    await client.waitIdle(1, bindings);
    assert.deepEqual(receipts, [10n, 20n]);
    await client.close();
  } finally {
    if (original === undefined) Reflect.deleteProperty(globalThis, "Worker");
    else Object.defineProperty(globalThis, "Worker", original);
  }
});
