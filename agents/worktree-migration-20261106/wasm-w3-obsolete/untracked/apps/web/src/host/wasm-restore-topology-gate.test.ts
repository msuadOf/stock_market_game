import assert from "node:assert/strict";
import test from "node:test";
import { commandDayEndArchiveFixture } from "../app/command-host-test-fixture.ts";
import { drain, fixture } from "./worker-host-w1.test-support.ts";
import { createFacadeEngineInitializer } from "./facade-engine-initialize.ts";

test("真实restore topology ABI缺失时main在Worker/Channel分配前带结构化原因拒绝", async () => {
  const f = fixture();
  try {
    f.launch({ onSpawnFailure() {}, initialArchive: commandDayEndArchiveFixture() }); await drain();
    const outcome = f.readOutcome(); assert.equal(outcome.status, "rejected");
    if (outcome.status !== "rejected") throw new Error("预期缺能力拒绝");
    const error = outcome.error as Error & { code: string; where: string; context: unknown };
    assert.equal(error.code, "WASM_RESTORE_TOPOLOGY_NOT_READY"); assert.match(error.message, /restore.*topology|topology.*restore/);
    assert.equal(error.where, "worker-host.initial-archive"); assert.deepEqual(error.context, { operation: "restore" });
    assert.equal(f.workers.length, 0); assert.equal(f.channels.length, 0);
  } finally { await f.cleanup(); }
});
test("已实例化的下层glue缺String ABI时显式拒绝，旧零分配stub合同已由外门取代", async () => {
  const channel = new MessageChannel(); const calls: string[] = [];
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const initializer = createFacadeEngineInitializer({
    isolated: () => true, createMemory: () => { calls.push("allocate"); return memory; },
    environment: {
      bindFacadeThreadEnvironment() { calls.push("bind"); },
      installFacadeSlotLayout() { calls.push("install"); },
      getArenaView() { calls.push("arena"); throw new Error("缺能力时不得读取arena"); },
    },
    load: async () => { calls.push("load"); return { module: new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), diagnostics: false, glue: {
      initSync: ({ memory: shared }: { memory: WebAssembly.Memory }) => { calls.push("instantiate"); return { memory: shared, __wbindgen_thread_destroy() {} }; },
      facade_slot_init_for_setup: () => { calls.push("setup-init"); },
      facade_slot_layout: () => { calls.push("layout"); throw new Error("缺能力时不得取layout"); },
    } }; }, onFailure: (error) => { throw error; },
  });
  channel.port1.addEventListener("message", () => assert.fail("缺能力时不得发送configured")); channel.port1.start();
  try {
    await assert.rejects(initializer.initialize({ type: "init", initialArchive: commandDayEndArchiveFixture(), controlPort: channel.port2 }), (error: unknown) => {
      assert.equal((error as { code: string }).code, "WASM_RESTORE_TOPOLOGY_NOT_READY");
      assert.equal((error as { where: string }).where, "facade-engine-initialize.initial-archive"); return true;
    });
    assert.deepEqual(calls, ["allocate", "bind", "load", "instantiate"]);
    assert.throws(() => initializer.requireConfigured(), /尚未 configured/);
  } finally { initializer.dispose(); channel.port1.close(); channel.port2.close(); }
});
