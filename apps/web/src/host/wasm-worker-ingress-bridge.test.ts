import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";
import { snapshot, tickBatch, frame } from "./protocol-test-fixtures.ts";
import { CAPACITY, GEN_BITS, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE } from "./facade-slot-protocol.ts";

/**
 * ingress 桥共享 substrate 门禁；W1 已批准从旧串行 fallback 迁移为显式失败。
 *
 * 背景：postIngressBridge 曾把非共享 WebAssembly.Memory 直接 postMessage 回
 * 主线程，浏览器按 structured clone 算法同步抛
 * `DataCloneError: #<Memory> could not be cloned`，令 main 本地 wasm 起局
 * 即崩（证据：.worktree/wasm-p0/agents/wasm-p0-web-recovery/record.md）。
 *
 * Node structuredClone 只验证消息合同；真实 transfer/关闭义务仍需产品浏览器验收。
 */
type Message = Record<string, unknown> & { type: string };
let fixtureId = 0;
const bindingsKey = Symbol.for("stock-game-wasm-ingress-bridge-tests");

interface FixtureOptions {
  /** true=合法共享产物；false=错误的非共享产物。 */
  readonly sharedMemory: boolean;
  /** bindings 是否导出 ingress_token（旧 bindings 可能缺失）。 */
  readonly withIngressToken: boolean;
}

async function withWorker(
  options: FixtureOptions,
  run: (fixture: Awaited<ReturnType<typeof createFixture>>) => Promise<void> | void,
) {
  const globals = new Map<PropertyKey, PropertyDescriptor | undefined>();
  const channels: MessageChannel[] = [], OriginalChannel = MessageChannel;
  const replace = (key: PropertyKey, value: unknown) => {
    globals.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, value });
  };
  let hooks: ReturnType<typeof registerHooks> | undefined;
  try {
    replace("MessageChannel", class extends OriginalChannel { constructor() { super(); channels.push(this); } });
    const fixture = await createFixture(options, replace, (registered) => { hooks = registered; });
    await run(fixture);
  } finally {
    for (const channel of channels) { channel.port1.close(); channel.port2.close(); }
    hooks?.deregister();
    for (const [key, original] of globals) {
      if (original === undefined) Reflect.deleteProperty(globalThis, key);
      else Object.defineProperty(globalThis, key, original);
    }
  }
}

async function createFixture(
  options: FixtureOptions,
  replace: (key: PropertyKey, value: unknown) => void,
  register: (hooks: ReturnType<typeof registerHooks>) => void,
) {
  const posted: Message[] = [];
  let initialized: () => void = () => { throw new Error("初始化信号未安装"); };
  const ready = new Promise<void>((resolve) => { initialized = resolve; });
  const timers = new Map<number, () => void>();
  let nextTimer = 0;
  const handle = 7;
  let listener: ((event: { data: Message }) => void) | undefined;
  const memory = options.sharedMemory
    ? new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true })
    : new WebAssembly.Memory({ initial: 1, maximum: 1 });
  const capabilities = { npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false }, personalTradeHistory: false };
  let boundMemory!: WebAssembly.Memory;
  const bindings: Record<string, unknown> = {
    initSync: (input: { memory: WebAssembly.Memory }) => { boundMemory = input.memory; return { memory: options.sharedMemory ? boundMemory : memory, __wbindgen_thread_destroy() {} }; },
    facade_slot_init_for_setup: () => { const words = new Int32Array(boundMemory.buffer); Atomics.store(words, CAPACITY, 4); Atomics.store(words, GEN_BITS, 29); },
    facade_slot_layout: () => ({ baseAddr: 0, headerWords: HEADER_WORDS, slotStride: SLOT_STRIDE, joinStride: JOIN_STRIDE, capacity: 4, genBits: 29 }),
    create_session: () => handle,
    civil_date: (current: number) => `2030-01-0${current}`,
    snapshot: (current: number) => ({ handle: current }),
    prepare_public_baseline: () => {},
    stock_catalog_view: () => [],
    company_catalog_view: () => [],
    host_capabilities: () => capabilities,
    step: () => tickBatch([frame(1, 0, [0])], snapshot(1, 1)),
    tick: () => 1n,
  };
  if (options.withIngressToken) bindings.ingress_token = (current: number) => current;
  replace(bindingsKey, bindings);
  replace("self", {
    crossOriginIsolated: true,
    // 浏览器 Worker#postMessage = structured clone 算法：不可克隆值同步抛
    // DataCloneError。此处先 structuredClone 再入账，复现该语义。
    postMessage: (message: Message) => { structuredClone(message); posted.push(message); if (message.type === "ready" || message.type === "failure") initialized(); },
    addEventListener: (_type: string, callback: typeof listener) => { listener = callback; },
  });
  replace("fetch", async () => ({ ok: true, arrayBuffer: async () => Uint8Array.from([0, 97, 115, 109, 1, 0, 0, 0]).buffer }));
  replace("setTimeout", (callback: () => void) => { const id = ++nextTimer; timers.set(id, callback); return id; });
  replace("clearTimeout", (id: number) => { timers.delete(id); });
  const id = ++fixtureId;
  register(registerHooks({
    resolve(specifier, context, nextResolve) {
      // 虚拟 wasm-pkg 模块短路：wasm-pkg 为构建产物，裸 worktree 可能缺失，
      // 若走 nextResolve 会对不存在的文件抛 ERR_MODULE_NOT_FOUND 令测试
      // 环境性红。fixture 自持虚拟模块，不依赖磁盘上的 wasm-pkg。
      if (specifier === "../../wasm-pkg/web_wasm.js") {
        return { url: `virtual:wasm-pkg:ingress-bridge-${id}`, shortCircuit: true };
      }
      if (specifier === "./engine-clock-env.ts") return { url: `virtual:ingress-env:${id}`, shortCircuit: true };
      return nextResolve(specifier, context);
    },
    load(url, context, nextLoad) {
      if (url.startsWith("virtual:wasm-pkg:ingress-bridge-")) {
        const exports = Object.keys(bindings)
          .map((key) => `export const ${key} = (...args) => bindings.${key}(...args);`).join("\n");
        return { shortCircuit: true, format: "module", source: `const bindings = globalThis[Symbol.for("stock-game-wasm-ingress-bridge-tests")];\n${exports}` };
      }
      if (url.startsWith("virtual:ingress-env:")) return { shortCircuit: true, format: "module", source: `import {createFacadeThreadBinding} from ${JSON.stringify(new URL("./facade-thread-binding.ts", import.meta.url).href)}; const binding=createFacadeThreadBinding({isWorker:()=>true}); export const {bindFacadeThreadEnvironment,installFacadeSlotLayout,getArenaView}=binding;` };
      const loaded = nextLoad(url, context);
      if (url.includes("/wasm-worker.ts?ingress-bridge-test=")) {
        // 仅替换 Node 缺少的 Vite import.meta.env；协议 listener 使用原源码。
        return { ...loaded, source: String(loaded.source).replaceAll("import.meta.env", '({ MODE: "e2e", DEV: false })') };
      }
      return loaded;
    },
  }));
  await import(`./wasm-worker.ts?ingress-bridge-test=${id}`);
  assert.ok(listener);
  const send = (message: Message) => { listener!({ data: message }); };
  const control = new MessageChannel(); let substrate: Record<string, unknown> | undefined;
  control.port1.addEventListener("message", (event) => { substrate = event.data; control.port1.postMessage({ type: "facadeEngineConfigured" }); }); control.port1.start();
  send({ type: "init", setup: {}, controlPort: control.port2 });
  await ready;
  const initialization = [...posted];
  if (options.sharedMemory) assert.deepEqual(posted, [{ type: "ready" }]);
  posted.length = 0;
  return { send, posted: posted as Message[], initialization, substrate };
}

test("非共享产物初始化显式失败，不交半个 ready/create/baseline", { timeout: 10000 }, async () => {
  await withWorker({ sharedMemory: false, withIngressToken: true }, async (f) => {
    assert.equal(f.initialization.length, 1); assert.equal(f.initialization[0].type, "failure");
    assert.match(String(f.initialization[0].message), /memory/);
    assert.equal(f.substrate, undefined);
  });
});

test("非共享产物且缺 ingress_token 不能静默降级到主通道", { timeout: 10000 }, async () => {
  await withWorker({ sharedMemory: false, withIngressToken: false }, async (f) => {
    assert.equal(f.initialization[0].type, "failure"); assert.equal(f.initialization.some((item) => item.type === "ready"), false);
  });
});

test("SharedArrayBuffer 不暴露时显式说明隔离能力错误，不能裸 ReferenceError", { timeout: 10000 }, async () => {
  // 浏览器非隔离上下文不暴露 SharedArrayBuffer 全局：探测 substrate 若直接
  // `buffer instanceof SharedArrayBuffer`，求值本身抛
  // ReferenceError: SharedArrayBuffer is not defined（浏览器实测，见
  // agents/w3-bridge-fix 台账）——必须先确认标识符存在再 instanceof。
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "SharedArrayBuffer");
  assert.ok(descriptor, "Node 环境应存在 SharedArrayBuffer 全局");
  Reflect.deleteProperty(globalThis, "SharedArrayBuffer");
  try {
    await withWorker({ sharedMemory: false, withIngressToken: true }, async (f) => {
      assert.equal(f.initialization[0].type, "failure");
      assert.match(String(f.initialization[0].message), /crossOriginIsolated|SharedArrayBuffer/);
      assert.doesNotMatch(String(f.initialization[0].message), /is not defined/);
    });
  } finally {
    Object.defineProperty(globalThis, "SharedArrayBuffer", descriptor);
  }
});

test("共享 module/memory 由一次控制握手交main，ingress仅带本代token", { timeout: 10000 }, async () => {
  await withWorker({ sharedMemory: true, withIngressToken: true }, async (f) => {
    f.send({ type: "create", seed: 1n });
    await Promise.resolve();
    const ingress = f.posted[0];
    assert.deepEqual(
      ingress,
      { type: "ingress", generation: 1, shared: true, token: 7 },
    );
    assert.ok(f.substrate);
    assert.ok(f.substrate.module instanceof WebAssembly.Module);
    assert.ok((f.substrate.memory as WebAssembly.Memory).buffer instanceof SharedArrayBuffer);
    // fixture 的 postMessage 已内置 structuredClone：消息入账即证明可克隆。
    assert.equal(f.posted[1].type, "created");
    assert.equal(f.posted[2].type, "baseline");
    assert.equal(f.posted.filter((message) => message.type === "failure").length, 0);
  });
});
