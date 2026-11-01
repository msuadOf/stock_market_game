import assert from "node:assert/strict";
import test from "node:test";
import { createFacadeEngineInitializer } from "./facade-engine-initialize.ts";
import { currentSaveFixture } from "../save/current-save-fixture.ts";
import { HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

function dayEndArchive(): Record<string, unknown> {
  const raw = currentSaveFixture();
  return { ...raw, civil_clock: { ...(raw.civil_clock as Record<string, unknown>), settled_through: "2029-12-31" } };
}

type FixtureOptions = {
  readonly hasRestore?: boolean;
  readonly clear?: () => void;
  readonly restore?: (json: string) => void;
  readonly setup?: (setup: unknown) => void;
};
function fixture(options: FixtureOptions = {}) {
  const channel = new MessageChannel(); const calls: string[] = []; const setups: unknown[] = []; const archives: unknown[] = [];
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 };
  const initializer = createFacadeEngineInitializer({
    isolated: () => true, createMemory: () => { calls.push("allocate"); return memory; },
    environment: {
      bindFacadeThreadEnvironment() { calls.push("bind"); },
      installFacadeSlotLayout() { calls.push("install"); },
      getArenaView: () => { calls.push("arena"); return { words: new Int32Array(memory.buffer), capacity: 4, genBits: 29 }; },
    },
    load: async () => { calls.push("load"); return { module: new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), diagnostics: false, glue: {
      initSync: ({ memory: shared }: { memory: WebAssembly.Memory }) => { calls.push("instantiate"); return { memory: shared, __wbindgen_thread_destroy() {} }; },
      facade_slot_init_for_setup: (setup: unknown) => { calls.push("setup-init"); setups.push(setup); options.setup?.(setup); },
      ...(options.hasRestore === false ? {} : { facade_slot_init_for_restore_json: (json: string) => { calls.push("restore-string-init"); archives.push(JSON.parse(json)); options.restore?.(json); } }),
      clear_prepared_restore: () => options.clear?.(),
      facade_slot_layout: () => { calls.push("layout"); return layout; },
    } }; }, onFailure: (error) => { throw error; },
  });
  channel.port1.addEventListener("message", (event) => channel.port1.postMessage(event.data.type === "facadeEngineMemory" ? { type: "facadeEngineMemoryRegistered", allocationId: 1 } : { type: "facadeEngineConfigured" })); channel.port1.start();
  return { initializer, channel, calls, setups, archives, close() { initializer.dispose(); channel.port1.close(); channel.port2.close(); } };
}
test("初始档沿String ABI合同消费同一完整X，绝不调用default-new initializer", async () => {
  const f = fixture(); const archive = dayEndArchive(); const expected = structuredClone(archive);
  try {
    const pending = f.initializer.initialize({ type: "init", initialArchive: archive, controlPort: f.channel.port2 });
    archive.seed = "999";
    await pending;
    const configured = f.initializer.requireConfigured();
    assert.deepEqual(f.archives, [expected]); assert.deepEqual(configured.setup, expected.setup);
    assert.deepEqual(configured.initialArchive, expected);
    assert.deepEqual(f.calls, ["allocate", "bind", "load", "instantiate", "restore-string-init", "layout", "install", "arena"]);
    assert.deepEqual(f.setups, []);
  } finally { f.close(); }
});
test("初始档缺少String ABI时在实例化后显式拒绝，且不layout/configure或调用new initializer", async () => {
  const f = fixture({ hasRestore: false });
  try {
    await assert.rejects(f.initializer.initialize({ type: "init", initialArchive: dayEndArchive(), controlPort: f.channel.port2 }), (error: unknown) => {
      assert.equal((error as { code: string }).code, "WASM_RESTORE_TOPOLOGY_NOT_READY");
      assert.equal((error as { where: string }).where, "facade-engine-initialize.initial-archive"); return true;
    });
    assert.deepEqual(f.calls, ["allocate", "bind", "load", "instantiate"]); assert.deepEqual(f.setups, []); assert.deepEqual(f.archives, []);
    assert.throws(() => f.initializer.requireConfigured(), /尚未 configured/);
  } finally { f.close(); }
});
test("String ABI原样抛出JsValue错误并清理，不切换到new initializer", async () => {
  const original = "OperationError: restore topology rejected"; const f = fixture({ restore: () => { throw original; } });
  try {
    await assert.rejects(f.initializer.initialize({ type: "init", initialArchive: dayEndArchive(), controlPort: f.channel.port2 }), (error) => error === original);
    await assert.rejects(f.initializer.initialize({ type: "init", setup: { retry: true }, controlPort: f.channel.port2 }), /重复|dispose/);
    assert.deepEqual(f.calls, ["allocate", "bind", "load", "instantiate", "restore-string-init"]); assert.deepEqual(f.setups, []);
    assert.throws(() => f.initializer.requireConfigured(), /尚未 configured/);
  } finally { f.close(); }
});
test("String ABI结构化错误保留原字段，且不切换到new initializer", async () => {
  const original = Object.assign(new Error("prefix failed"), { code: "RESTORE_PREFIX", where: "restore_runtime_topology", context: { stocks: 3 } });
  const f = fixture({ restore: () => { throw original; } });
  try {
    await assert.rejects(f.initializer.initialize({ type: "init", initialArchive: dayEndArchive(), controlPort: f.channel.port2 }), (error) => {
      assert.equal(error, original); assert.equal((error as typeof original).code, "RESTORE_PREFIX");
      assert.equal((error as typeof original).where, "restore_runtime_topology"); assert.deepEqual((error as typeof original).context, { stocks: 3 }); return true;
    });
    assert.deepEqual(f.setups, []); assert.deepEqual(f.calls, ["allocate", "bind", "load", "instantiate", "restore-string-init"]);
  } finally { f.close(); }
});
test("初始档与setup并存必须在新memory分配前拒绝", async () => {
  const f = fixture();
  try {
    await assert.rejects(f.initializer.initialize({ type: "init", initialArchive: dayEndArchive(), setup: {}, controlPort: f.channel.port2 }), /互斥|唯一/);
    assert.deepEqual(f.calls, []); assert.deepEqual(f.setups, []);
  } finally { f.close(); }
});
test("非法初始档保留校验原因，拒绝发生于新memory分配之前", async () => {
  const f = fixture();
  try {
    await assert.rejects(f.initializer.initialize({ type: "init", initialArchive: { ...dayEndArchive(), seed: "坏seed" }, controlPort: f.channel.port2 }), /seed/);
    assert.deepEqual(f.calls, []); assert.deepEqual(f.setups, []);
  } finally { f.close(); }
});
test("普通新局仍只用原setup，配置结果明确无初始档", async () => {
  const f = fixture(); const setup = { stocks: ["synthetic"] };
  try {
    await f.initializer.initialize({ type: "init", setup, controlPort: f.channel.port2 });
    const configured = f.initializer.requireConfigured();
    assert.equal(configured.initialArchive, null); assert.deepEqual(configured.setup, setup);
    assert.deepEqual(f.calls, ["allocate", "bind", "load", "instantiate", "setup-init", "layout", "install", "arena"]);
  } finally { f.close(); }
});
test("初始档必须是日终静点，schema合法的日内档也在memory前拒绝", async () => {
  const f = fixture();
  try {
    await assert.rejects(f.initializer.initialize({ type: "init", initialArchive: currentSaveFixture(), controlPort: f.channel.port2 }), /日终/);
    assert.deepEqual(f.calls, []); assert.deepEqual(f.setups, []);
  } finally { f.close(); }
});
test("初始化完成但未消费时 dispose 会释放 Rust 预检档", { timeout: 10000 }, async () => {
  let cleared = 0;
  const f = fixture({ clear: () => { cleared += 1; } });
  try {
    await f.initializer.initialize({ type: "init", initialArchive: dayEndArchive(), controlPort: f.channel.port2 });
    f.initializer.dispose();
    assert.equal(cleared, 1);
  } finally { f.close(); }
});
