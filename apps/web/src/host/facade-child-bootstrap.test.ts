import assert from "node:assert/strict";
import { test } from "node:test";
import { createFacadeChildBootstrap, parseFacadeChildBootstrap } from "./facade-child-bootstrap.ts";
import type { FacadeChildControlMessage, FacadeChildGlue } from "./facade-child-bootstrap.ts";
import { createFacadeThreadBinding } from "./facade-thread-binding.ts";
import { CAPACITY, GEN_BITS, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

const module = new WebAssembly.Module(new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0]));
function fixture(options: { result?: number; intent?: "missing" | "wrong" | "duplicate"; destroyFails?: boolean; postFails?: string; load?: () => Promise<FacadeChildGlue> } = {}) {
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const words = new Int32Array(memory.buffer);
  words[CAPACITY] = 2; words[GEN_BITS] = 29;
  const layout = { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 2, genBits: 29 };
  const channel = new MessageChannel();
  channel.port1.close(); channel.port2.close();
  const input = { type: "facadeChildBootstrap" as const, module, memory, layout, workerId: 9, slot: 1, gen: 3, diagnostics: false, controlPort: channel.port1 };
  const events: string[] = [];
  const messages: FacadeChildControlMessage[] = [];
  const errors: Error[] = [];
  const environment = createFacadeThreadBinding({ isWorker: () => true });
  let destroyed = false;
  const glue: FacadeChildGlue = {
    initSync(actual) { assert.deepEqual(actual, { module, memory }); events.push("instantiate"); return { memory, __wbindgen_thread_destroy(...args) { assert.deepEqual(args, [0, 0, 0]); events.push("destroy"); destroyed = true; if (options.destroyFails) throw new Error("destroy 原错误"); } }; },
    facade_slot_layout() { assert.equal(destroyed, false); events.push("layout"); return layout; },
    facade_child_trampoline(slot, gen, workerId) {
      assert.equal(destroyed, false); events.push("trampoline");
      if (options.intent !== "missing") environment.facade_worker_exit_notify(options.intent === "wrong" ? 10 : workerId, slot, gen);
      if (options.intent === "duplicate") environment.facade_worker_exit_notify(workerId, slot, gen);
      return options.result === undefined ? 0 : options.result;
    },
  };
  const runner = createFacadeChildBootstrap({ environment, loadGlue: options.load === undefined ? async () => glue : options.load,
    postControl(message) { events.push(message.type); if (message.type === options.postFails) throw new Error("postMessage 原错误"); messages.push(message); },
    postValidationFailure(message) { if (message.type === options.postFails) throw new Error("全局 postMessage 原错误"); messages.push(message); events.push(message.type); },
    reportError(error) { errors.push(error); events.push("reportError"); }, close() { events.push("close"); },
  });
  return { input, events, messages, errors, runner, glue };
}

for (const result of [0, 1]) test(`trampoline ${result}：ready先发、destroy最后WASM、exit和close各一次`, async () => {
  const f = fixture({ result }); await f.runner.handleMessage(f.input);
  assert.deepEqual(f.events, ["instantiate", "layout", "facadeChildReady", "trampoline", "destroy", "facadeWorkerExit", "close"]);
  assert.equal(f.runner.isFinished(), true);
  await f.runner.handleMessage(f.input);
  assert.equal(f.events.filter((event) => event === "trampoline").length, 1);
  assert.deepEqual(f.messages.at(-1), { type: "facadeWorkerExit", workerId: 9, slot: 1, gen: 3 });
});

for (const intent of ["missing", "wrong", "duplicate"] as const) test(`退出意图${intent}显式失败，不能destroy或成功exit`, async () => {
  const f = fixture({ intent }); await f.runner.handleMessage(f.input);
  assert.equal(f.events.includes("destroy"), false);
  assert.equal(f.events.includes("facadeWorkerExit"), false);
  const failure = f.messages.at(-1);
  assert.equal(failure?.type, "facadeBootstrapFailure");
  if (failure?.type === "facadeBootstrapFailure") assert.match(failure.failure.message, /exit|身份|重复/);
});

test("非法trampoline返回码显式失败", async () => {
  const f = fixture({ result: 2 }); await f.runner.handleMessage(f.input);
  const message = f.messages.at(-1);
  assert.equal(message?.type, "facadeBootstrapFailure");
  if (message?.type === "facadeBootstrapFailure") assert.equal(message.phase, "cleanup");
  assert.equal(f.events.includes("destroy"), false);
});

test("DONE后destroy失败有独立HostFailure，保留原错误且不发成功exit", async () => {
  const f = fixture({ destroyFails: true }); await f.runner.handleMessage(f.input);
  const failure = f.messages.at(-1);
  assert.equal(failure?.type, "facadeBootstrapFailure");
  if (failure?.type === "facadeBootstrapFailure") { assert.equal(failure.phase, "cleanup"); assert.match(failure.failure.message, /destroy 原错误/); assert.equal(failure.failure.code, "FACADE_BOOTSTRAP_FAILURE"); }
  assert.equal(f.events.includes("facadeWorkerExit"), false);
  assert.equal(f.events.at(-1), "close");
});

test("错误消息也不能投递时reportError保留cleanup和post两错", async () => {
  const f = fixture({ destroyFails: true, postFails: "facadeBootstrapFailure" }); await f.runner.handleMessage(f.input);
  assert.equal(f.errors.length, 1); assert.ok(f.errors[0] instanceof AggregateError);
  assert.match(String(f.errors[0].errors[0]), /destroy 原错误/);
  assert.match(String(f.errors[0].errors[1]), /postMessage 原错误/);
  assert.match(f.errors[0].message, /\[FACADE_BOOTSTRAP_REPORTER_FAILURE\]/);
  assert.match(f.errors[0].message, /destroy 原错误/);
});

test("非法输入的validate failure没有伪造identity", async () => {
  const f = fixture(); await f.runner.handleMessage({ ...f.input, workerId: -1 });
  assert.deepEqual(f.events, ["facadeBootstrapFailure", "close"]);
  const failure = f.messages[0];
  assert.equal(failure.type, "facadeBootstrapFailure");
  assert.equal("workerId" in failure, false);
});

test("parser拒绝缺失module/shared memory/port/diagnostics及越界identity", () => {
  const f = fixture(); assert.ok(Object.isFrozen(parseFacadeChildBootstrap(f.input)));
  for (const patch of [{ module: null }, { memory: new WebAssembly.Memory({ initial: 1 }) }, { controlPort: {} }, { diagnostics: "false" }, { gen: 0x20000000 }, { slot: 2 }, { workerId: 2 ** 32 }, { workerId: -1 }]) assert.throws(() => parseFacadeChildBootstrap({ ...f.input, ...patch }));
});

test("异步加载时重复bootstrap显式失败，原任务恢复后零Rust执行", async () => {
  let resume!: (value: FacadeChildGlue) => void;
  const f = fixture({ load: () => new Promise((resolve) => { resume = resolve; }) });
  const first = f.runner.handleMessage(f.input);
  await f.runner.handleMessage(f.input);
  resume(f.glue); await first;
  assert.equal(f.events.includes("trampoline"), false);
  assert.equal(f.events.filter((event) => event === "facadeBootstrapFailure").length, 1);
  assert.equal(f.events.filter((event) => event === "close").length, 1);
});

test("畸形或迟到bootstrap携带的额外port须关闭且不执行Rust", async () => {
  const f = fixture();
  const extra = new MessageChannel(); let closed = 0;
  extra.port1.close = () => { closed++; };
  await f.runner.handleMessage({ ...f.input, workerId: -1, controlPort: extra.port1 });
  assert.equal(closed, 1);
  await f.runner.handleMessage({ ...f.input, controlPort: extra.port1 });
  assert.equal(closed, 1);
  extra.port2.close();
});

test("控制端故障保留全局可见failure出口", async () => {
  const f = fixture(); const globalMessages: unknown[] = [];
  const runner = createFacadeChildBootstrap({ environment: createFacadeThreadBinding({ isWorker: () => true }), loadGlue: async () => { throw new Error("instantiate 失败"); }, postControl() { throw new Error("控制投递失败"); }, postValidationFailure(message) { globalMessages.push(message); }, reportError() { assert.fail("全局投递成功无需reportError"); }, close() {} });
  await runner.handleMessage(f.input);
  assert.equal(globalMessages.length, 1);
  const message = globalMessages[0] as { failure: { message: string; context: unknown } };
  assert.match(message.failure.message, /instantiate 失败/);
  assert.match(JSON.stringify(message.failure.context), /控制投递失败/);
});

test("故障message getter只读一次，空或不可字符串化错误仍可见", async () => {
  for (const error of [new Error(""), { toString() { throw new Error("无法字符串化"); } }, Object.create(null)]) {
    const f = fixture({ load: async () => { throw error; } });
    await f.runner.handleMessage(f.input);
    const failure = f.messages.at(-1);
    assert.equal(failure?.type, "facadeBootstrapFailure");
    if (failure?.type === "facadeBootstrapFailure") assert.ok(failure.failure.message.trim().length > 0);
  }
  let reads = 0;
  const original = new Error("原错误");
  Object.defineProperty(original, "message", { get() { reads++; if (reads > 1) throw new Error("重复读取"); return "已读原错误"; } });
  const f = fixture({ load: async () => { throw original; } }); await f.runner.handleMessage(f.input);
  assert.equal(reads, 1);
  assert.match(JSON.stringify(f.messages.at(-1)), /已读原错误/);
});

test("畸形controlPort getter不能令catch清理绕过failure", async () => {
  const f = fixture();
  const input = { ...f.input, get controlPort() { throw new Error("恶意port getter"); } };
  await f.runner.handleMessage(input);
  assert.equal(f.messages.at(-1)?.type, "facadeBootstrapFailure");
  assert.equal(f.events.at(-1), "close");
});

test("重复bootstrap终结后先前loadGlue拒绝不得再投递旧port或再次关闭", async () => {
  let reject!: (error: unknown) => void;
  const f = fixture({ load: () => new Promise((_resolve, fail) => { reject = fail; }) });
  const first = f.runner.handleMessage(f.input);
  await f.runner.handleMessage(f.input);
  reject(new Error("迟到的加载失败")); await first;
  assert.equal(f.events.filter((event) => event === "facadeBootstrapFailure").length, 1);
  assert.equal(f.events.filter((event) => event === "close").length, 1);
  assert.equal(f.events.includes("trampoline"), false);
});
