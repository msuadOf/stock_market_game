import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";
import { CAPACITY, GEN_BITS, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";
import { WorkerTransportFixture } from "./worker-transport.test-support.ts";

let nextId = 0;
async function fixture(run: (f: { send(value: Record<string, unknown>): Promise<void>; posted: Record<string, unknown>[]; payload: Record<string, unknown>; calls: string[]; reported: Error[] }) => Promise<void>, postError?: Error, sharedMemory = true) {
  const id = ++nextId, key = Symbol.for(`stock-game-ingress-substrate-${id}`);
  const posted: Record<string, unknown>[] = [], lifecycle: Record<string, unknown>[] = [], calls: string[] = [], reported: Error[] = [];
  let listener!: (event: { data: Record<string, unknown> }) => void;
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, ...(sharedMemory ? { shared: true } : {}) }), words = new Int32Array(memory.buffer);
  if (sharedMemory) { Atomics.store(words, CAPACITY, 4); Atomics.store(words, GEN_BITS, 29); }
  const substrate = { diagnostics: false, module: new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), memory, layout: { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 } };
  const payload = { type: "bindIngress", requestId: 1, generation: 1, token: 7, shared: true };
  const transport = new WorkerTransportFixture();
  const originalSelf = Object.getOwnPropertyDescriptor(globalThis, "self");
  Object.defineProperty(globalThis, key, { configurable: true, value: { calls, bound: false, memory } });
  Object.defineProperty(globalThis, "self", { configurable: true, value: { postMessage(value: Record<string, unknown>) { if (value.type !== "ingressLifecycle") throw new Error("ingress business output bypassed dataPort"); lifecycle.push(value); }, reportError(error: Error) { reported.push(error); }, addEventListener(_type: string, callback: typeof listener) { listener = callback; } } });
  const hooks = registerHooks({
    resolve(specifier, context, next) {
      if (specifier === "../../wasm-pkg/web_wasm.js" || specifier === "./engine-clock-env.ts") return { url: `virtual:ingress-substrate:${id}:${specifier}`, shortCircuit: true };
      return next(specifier, context);
    },
    load(url, context, next) {
      const state = `globalThis[Symbol.for(${JSON.stringify(Symbol.keyFor(key))})]`;
      if (url.endsWith(`${id}:../../wasm-pkg/web_wasm.js`)) return { shortCircuit: true, format: "module", source: `const f=${state}; export function initSync(input){if(!f.bound)throw new Error("env not bound");if(input.memory!==f.memory)throw new Error("wrong memory"); f.calls.push("initSync");return {memory:input.memory,__wbindgen_thread_destroy(){}}}; export function ingress_enqueue(token,intent){f.calls.push(JSON.stringify([token,intent]))}` };
      if (url.endsWith(`${id}:./engine-clock-env.ts`)) return { shortCircuit: true, format: "module", source: `import {createFacadeThreadBinding} from ${JSON.stringify(new URL("./facade-thread-binding.ts", import.meta.url).href)};const b=createFacadeThreadBinding({isWorker:()=>true});const f=${state}; export function bindFacadeThreadEnvironment(input){b.bindFacadeThreadEnvironment(input);f.bound=true;f.calls.push(input.role)};export const {installFacadeSlotLayout,getArenaView}=b;` };
      return next(url, context);
    },
  });
  try {
    await import(`./wasm-ingress-worker.ts?substrate=${id}`);
    const ownerPort = transport.bindIngressTransport(listener as (event: MessageEvent<unknown>) => void, 1, substrate);
    const workerPort = transport.channels[0]!.port2 as unknown as MessagePort;
    const originalPost = workerPort.postMessage.bind(workerPort);
    workerPort.postMessage = (value: unknown) => { if (postError !== undefined) throw postError; originalPost(value); };
    ownerPort.addEventListener("message", (event) => posted.push((event as MessageEvent<Record<string, unknown>>).data));
    ownerPort.start();
    const flush = async () => { for (let count = 0; count < 8; count++) await new Promise((resolve) => setImmediate(resolve)); };
    await run({ posted, calls, payload, reported, async send(value) { ownerPort.postMessage(value); await flush(); } });
  } finally { transport.close(); hooks.deregister(); Reflect.deleteProperty(globalThis, key); if (originalSelf === undefined) Reflect.deleteProperty(globalThis, "self"); else Object.defineProperty(globalThis, "self", originalSelf); }
}

test("ingress 首次先bind角色再用engine共享module/memory实例化，换代只换token", { timeout: 10000 }, async () => {
  await fixture(async (f) => {
    await f.send(f.payload);
    assert.deepEqual(f.calls, ["ingress", "initSync"]);
    assert.equal(f.posted[0].type, "ingressBound");
    await f.send({ type: "enqueue", requestId: 2, generation: 1, intent: { Cancel: { code: 0, id: 7 } } });
    assert.equal(f.calls.at(-1), '[7,{"Cancel":{"code":0,"id":7}}]');
    await f.send({ type: "bindIngress", requestId: 3, generation: 2, token: 9, shared: true });
    assert.equal(f.calls.filter((call) => call === "initSync").length, 1);
    await f.send({ type: "enqueue", requestId: 4, generation: 1, intent: {} });
    assert.equal(f.posted.at(-1)?.type, "operationError"); assert.match(String(f.posted.at(-1)?.message), /已过期/);
  });
});

test("operationError 发送失败的 native 文本同时保留业务原因与发送原因", { timeout: 10000 }, async () => {
  await fixture(async (f) => {
    await f.send({ type: "bindIngress", generation: "bad", token: 0 });
    assert.equal(f.reported.length, 1);
    assert.match(f.reported[0].message, /generation/);
    assert.match(f.reported[0].message, /clone denied/);
    assert.equal((f.reported[0] as AggregateError).errors.length, 2);
  }, new DOMException("clone denied", "DataCloneError"));
});

test("ingress 非共享及字符串generation/token拒绝，不冒充成功绑定", { timeout: 10000 }, async () => {
  await fixture(async (f) => {
    await f.send({ ...f.payload, generation: "1", token: "7" });
    assert.equal(f.posted[0]?.type, "operationError"); assert.deepEqual(f.calls, []);
  });
});

test("非共享 substrate 在真实 ingress bootstrap 显式报告且不初始化 WASM", { timeout: 10000 }, async () => {
  await fixture(async (f) => {
    assert.equal(f.posted.length, 0);
    assert.deepEqual(f.calls, []);
    assert.equal(f.reported.length, 1);
    assert.match(f.reported[0].message, /shared|共享|memory/i);
  }, undefined, false);
});
