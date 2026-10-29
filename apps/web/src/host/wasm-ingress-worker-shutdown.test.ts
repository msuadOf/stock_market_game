import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";
import { CAPACITY, GEN_BITS, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

test("生产 ingress quiesce 在已排队 enqueue 后 ACK，之后 bind/enqueue 均不能再进 WASM", { timeout: 10000 }, async () => {
  const key = Symbol.for("stock-game-ingress-shutdown");
  const saved = new Map<PropertyKey, PropertyDescriptor | undefined>();
  function replace(name: PropertyKey, value: unknown) { saved.set(name, Object.getOwnPropertyDescriptor(globalThis, name)); Object.defineProperty(globalThis, name, { value, configurable: true }); }
  const posted: Record<string, unknown>[] = [], calls: unknown[] = [];
  let handler!: (event: { data: Record<string, unknown> }) => void;
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const words = new Int32Array(memory.buffer); Atomics.store(words, CAPACITY, 4); Atomics.store(words, GEN_BITS, 29);
  replace(key, { initSync: () => ({ memory, __wbindgen_thread_destroy() {} }), ingress_enqueue: (token: number, intent: unknown) => { calls.push({ token, intent }); } });
  replace("self", { postMessage: (message: Record<string, unknown>) => { posted.push(message); }, addEventListener(_type: string, listener: typeof handler) { handler = listener; } });
  const hooks = registerHooks({
    resolve(specifier, context, next) {
      if (specifier === "../../wasm-pkg/web_wasm.js") return { url: "virtual:ingress-shutdown-bindings", shortCircuit: true };
      if (specifier === "./engine-clock-env.ts") return { url: "virtual:ingress-shutdown-env", shortCircuit: true };
      return next(specifier, context);
    },
    load(url, context, next) {
      if (url === "virtual:ingress-shutdown-bindings") return { format: "module", shortCircuit: true, source: 'const f=globalThis[Symbol.for("stock-game-ingress-shutdown")]; export const initSync=f.initSync; export const ingress_enqueue=f.ingress_enqueue;' };
      if (url === "virtual:ingress-shutdown-env") return { format: "module", shortCircuit: true, source: `import {createFacadeThreadBinding} from ${JSON.stringify(new URL("./facade-thread-binding.ts", import.meta.url).href)}; const binding=createFacadeThreadBinding({isWorker:()=>true}); export const {bindFacadeThreadEnvironment,installFacadeSlotLayout,getArenaView}=binding;` };
      return next(url, context);
    },
  });
  const flush = async () => { for (let count = 0; count < 8; count++) await new Promise((resolve) => setImmediate(resolve)); };
  try {
    const workerUrl = "./wasm-ingress-worker.ts?shutdown-test";
    await import(workerUrl);
    handler({ data: { type: "bindIngress", requestId: 1, generation: 1, token: 7, shared: true, memory, module: new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), diagnostics: false,
      layout: { baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 } } });
    await flush(); assert.equal(posted.at(-1)?.type, "ingressBound");
    handler({ data: { type: "enqueue", requestId: 2, generation: 1, intent: { marker: 1 } } });
    handler({ data: { type: "quiesceIngress", requestId: 3, generation: 1 } });
    await flush();
    assert.deepEqual(calls, [{ token: 7, intent: { marker: 1 } }]);
    assert.deepEqual(posted.slice(-2).map((message) => message.type), ["enqueued", "ingressQuiescent"]);
    handler({ data: { type: "enqueue", requestId: 4, generation: 1, intent: { marker: 2 } } });
    handler({ data: { type: "bindIngress", requestId: 5, generation: 2, token: 8, shared: true } });
    await flush();
    assert.equal(calls.length, 1);
    assert.deepEqual(posted.slice(-2).map((message) => message.type), ["operationError", "operationError"]);
  } finally { hooks.deregister(); for (const [name, value] of saved) { if (value === undefined) Reflect.deleteProperty(globalThis, name); else Object.defineProperty(globalThis, name, value); } }
});
