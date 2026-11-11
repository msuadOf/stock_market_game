import assert from "node:assert/strict";
import test from "node:test";
import { createSha256Chain } from "../save/sha256-chain.ts";
import { createFacadeEngineInitializer } from "./facade-engine-initialize.ts";
import { sendRestoreChunks, RestoreSourceCleanupTimeoutError } from "./restore-chunk-port.ts";
import { HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

async function descriptor(bytes: Uint8Array) { const chain = await createSha256Chain(); await chain.update(bytes); return chain.finish(); }
async function* stream(bytes: Uint8Array) { yield bytes; }
function fixture(missing = false, runtime = false) {
  const control = new MessageChannel(), input = new MessageChannel(), events: string[] = [];
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 };
  const glue = { initSync: () => ({ memory, __wbindgen_thread_destroy() {} }), wasm_tombstone_init_primary: () => ({}),
    facade_slot_init_for_setup() { throw new Error("不能建新局"); }, facade_slot_layout() { events.push("layout"); return layout; },
    facade_restore_chunks_begin() { events.push("begin"); return 7; }, facade_restore_chunks_push(token: number) { assert.equal(token, 7); events.push("push"); },
    facade_restore_chunks_finish(token: number) { assert.equal(token, 7); events.push("finish"); }, facade_restore_chunks_clear(token: number) { assert.equal(token, 7); events.push("clear"); return true; },
    ...(missing ? {} : { restore_prepared_chunks() { return 9; } }),
    ...(runtime ? {set_step_runtime_override() {}} : {}),
  };
  const initializer = createFacadeEngineInitializer({ isolated: () => true, createMemory: () => memory,
    environment: { bindFacadeThreadEnvironment() {}, installFacadeSlotLayout() {}, getArenaView: () => ({ words: new Int32Array(memory.buffer), capacity: 4, genBits: 29 }) },
    load: async () => ({ module: new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), diagnostics: false, glue }), onFailure(error) { throw error; } });
  control.port1.addEventListener("message", event => { events.push(event.data.type); control.port1.postMessage(event.data.type === "facadeEngineMemory" ? { type: "facadeEngineMemoryRegistered", allocationId: 1 } : event.data.type === "facadeEngineDeath" ? { type: "facadeEngineDeathRegistered", allocationId: 1 } : { type: "facadeEngineConfigured" }); }); control.port1.start();
  return { initializer, control, input, events, glue, close() { initializer.dispose(); control.port1.close(); control.port2.close(); input.port1.close(); input.port2.close(); } };
}

test("分块初始档缺少正式 LibOS 导出时清理已准备 token", {timeout:10000}, async () => {
  const f = fixture(false, true), bytes = new TextEncoder().encode("fixture"), contentDigest = await descriptor(bytes);
  let inputCloses = 0, controlCloses = 0;
  const closeInput = f.input.port2.close.bind(f.input.port2), closeControl = f.control.port2.close.bind(f.control.port2);
  f.input.port2.close = () => { inputCloses += 1; closeInput(); };
  f.control.port2.close = () => { controlCloses += 1; closeControl(); };
  try {
    const pending = f.initializer.initialize({type:"init", controlPort:f.control.port2,
      initialChunked:{port:f.input.port2,generation:1,contentDigest,availableWasmBytes:8 * 1024 * 1024},
      runtimeOverride:true});
    const sender = sendRestoreChunks(f.input.port1, 1, bytes.length, stream(bytes));
    await assert.rejects(pending, /LibOS.*故障.*导出/);
    await sender;
    assert.equal(f.events.filter(value => value === "clear").length, 1);
    assert.ok(inputCloses >= 1);
    assert.equal(controlCloses, 1);
    assert.ok(!f.events.includes("facadeEngineConfiguration"));
    assert.throws(() => f.initializer.requireConfigured(), /尚未 configured|dispose/);
  } finally { f.close(); }
});

test("chunked 在完成接收和摘要后 configured，token 供唯一消费", { timeout: 10000 }, async () => {
  const f = fixture(), bytes = new TextEncoder().encode("fixture"), contentDigest = await descriptor(bytes);
  try {
    const initialized = f.initializer.initialize({ type: "init", controlPort: f.control.port2, initialChunked: { port: f.input.port2, generation: 1, contentDigest, availableWasmBytes: 8 * 1024 * 1024 } });
    const sending = sendRestoreChunks(f.input.port1, 1, bytes.length, stream(bytes));
    await Promise.all([initialized, sending]);
    assert.equal(f.initializer.requireConfigured().chunkToken, 7);
    assert.ok(f.events.indexOf("finish") < f.events.indexOf("facadeEngineConfiguration"));
  } finally { f.close(); }
});

test("分块 ABI 缺项不能 configured，摘要错误不能创建候选", { timeout: 10000 }, async () => {
  const bytes = new TextEncoder().encode("fixture"), contentDigest = await descriptor(bytes);
  for (const missing of [true, false]) {
    const f = fixture(missing);
    try {
      const pending = f.initializer.initialize({ type: "init", controlPort: f.control.port2, initialChunked: { port: f.input.port2, generation: 1, contentDigest, availableWasmBytes: 8 * 1024 * 1024 } });
      const send = missing ? Promise.resolve() : sendRestoreChunks(f.input.port1, 1, bytes.length, stream(new TextEncoder().encode("fixturE")));
      const settled = Promise.allSettled([send]);
      await assert.rejects(pending); await settled;
      assert.ok(!f.events.includes("facadeEngineConfiguration")); assert.throws(() => f.initializer.requireConfigured(), /尚未 configured|dispose/);
    } finally { f.close(); }
  }
});

test("取消初始化等待 receiver 清理；源不结束须单独报告 unknown", { timeout: 10000 }, async () => {
  const f = fixture(), bytes = new TextEncoder().encode("fixture"), contentDigest = await descriptor(bytes);
  let sourceStarted!: () => void; const started = new Promise<void>(resolve => { sourceStarted = resolve; });
  async function* blocked() { sourceStarted(); await new Promise(() => {}); yield bytes; }
  try {
    const pending = f.initializer.initialize({ type: "init", controlPort: f.control.port2, initialChunked: { port: f.input.port2, generation: 1, contentDigest, availableWasmBytes: 8 * 1024 * 1024 } });
    const failedInit = assert.rejects(pending);
    const sender = sendRestoreChunks(f.input.port1, 1, bytes.length, blocked());
    const failedSender = assert.rejects(sender, error => error instanceof RestoreSourceCleanupTimeoutError);
    await started;
    await f.initializer.disposeAsync();
    await Promise.all([failedInit, failedSender]);
    assert.ok(!f.events.includes("finish")); assert.ok(!f.events.includes("facadeEngineConfiguration"));
  } finally { f.close(); }
});

test("finish 同步触发取消时只清所属 token，不发布 configured", { timeout: 10000 }, async () => {
  const f = fixture(), bytes = new TextEncoder().encode("fixture"), contentDigest = await descriptor(bytes);
  const finish = f.glue.facade_restore_chunks_finish;
  f.glue.facade_restore_chunks_finish = (token: number) => { finish(token); f.initializer.dispose(); };
  try {
    const pending = f.initializer.initialize({ type: "init", controlPort: f.control.port2, initialChunked: { port: f.input.port2, generation: 1, contentDigest, availableWasmBytes: 8 * 1024 * 1024 } });
    const sender = sendRestoreChunks(f.input.port1, 1, bytes.length, stream(bytes)); const settled = Promise.allSettled([sender]);
    await assert.rejects(pending); await settled;
    assert.equal(f.events.filter(value => value === "clear").length, 1);
    assert.ok(!f.events.includes("facadeEngineConfiguration"));
  } finally { f.close(); }
});

test("token 清理失败仍关闭 handshake 并保留原错", { timeout: 10000 }, async () => {
  const f = fixture(), bytes = new TextEncoder().encode("fixture"), contentDigest = await descriptor(bytes), failure = new Error("清理失败");
  try {
    const pending = f.initializer.initialize({ type: "init", controlPort: f.control.port2, initialChunked: { port: f.input.port2, generation: 1, contentDigest, availableWasmBytes: 8 * 1024 * 1024 } });
    await Promise.all([pending, sendRestoreChunks(f.input.port1, 1, bytes.length, stream(bytes))]);
    f.glue.facade_restore_chunks_clear = () => { throw failure; };
    await assert.rejects(f.initializer.disposeAsync(), error => error === failure);
    assert.throws(() => f.initializer.requireConfigured(), /dispose/);
  } finally { f.close(); }
});
