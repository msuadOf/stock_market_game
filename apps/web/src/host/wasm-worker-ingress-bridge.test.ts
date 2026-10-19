import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";
import { snapshot, tickBatch, frame } from "./protocol-test-fixtures.ts";

/**
 * ingress 桥 substrate 兼容性测试（W3 遗留断裂红证）。
 *
 * 背景：postIngressBridge 曾把非共享 WebAssembly.Memory 直接 postMessage 回
 * 主线程，浏览器按 structured clone 算法同步抛
 * `DataCloneError: #<Memory> could not be cloned`，令 main 本地 wasm 起局
 * 即崩（证据：.worktree/wasm-p0/agents/wasm-p0-web-recovery/record.md）。
 *
 * 本文件在 Node 中用 `structuredClone` 复现浏览器 postMessage 的克隆语义
 * （Worker#postMessage 即 structured clone 算法），并按两种 substrate 分别
 * 验证：现行串行构建（非共享 memory）与未来真线程构建（共享 memory）。
 */
type Message = Record<string, unknown> & { type: string };
let fixtureId = 0;
const bindingsKey = Symbol.for("stock-game-wasm-ingress-bridge-tests");

interface FixtureOptions {
  /** true=真线程形态（SharedArrayBuffer 背衬）；false=现行串行构建。 */
  readonly sharedMemory: boolean;
  /** bindings 是否导出 ingress_token（旧 bindings 可能缺失）。 */
  readonly withIngressToken: boolean;
}

async function withWorker(
  options: FixtureOptions,
  run: (fixture: Awaited<ReturnType<typeof createFixture>>) => Promise<void> | void,
) {
  const globals = new Map<PropertyKey, PropertyDescriptor | undefined>();
  const replace = (key: PropertyKey, value: unknown) => {
    globals.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, value });
  };
  let hooks: ReturnType<typeof registerHooks> | undefined;
  try {
    const fixture = await createFixture(options, replace, (registered) => { hooks = registered; });
    await run(fixture);
  } finally {
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
  const bindings: Record<string, unknown> = {
    default: async () => ({ memory }),
    create_session: () => handle,
    civil_date: (current: number) => `2030-01-0${current}`,
    snapshot: (current: number) => ({ handle: current }),
    prepare_public_baseline: () => {},
    host_capabilities: () => capabilities,
    step: () => tickBatch([frame(1, 0, [0])], snapshot(1, 1)),
    tick: () => 1n,
  };
  if (options.withIngressToken) bindings.ingress_token = (current: number) => current;
  replace(bindingsKey, bindings);
  replace("self", {
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
      return nextResolve(specifier, context);
    },
    load(url, context, nextLoad) {
      if (url.startsWith("virtual:wasm-pkg:ingress-bridge-")) {
        const exports = Object.keys(bindings)
          .filter((key) => key !== "default")
          .map((key) => `export const ${key} = (...args) => bindings.${key}(...args);`).join("\n");
        return { shortCircuit: true, format: "module", source: `const bindings = globalThis[Symbol.for("stock-game-wasm-ingress-bridge-tests")];\n${exports}\nexport default bindings.default;` };
      }
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
  send({ type: "init" });
  await ready;
  assert.deepEqual(posted, [{ type: "ready" }]);
  posted.length = 0;
  return { send, posted: posted as Message[] };
}

test("串行 substrate（非共享 memory）create 不回传 Memory，起局不因 DataCloneError 崩溃", { timeout: 10000 }, async () => {
  await withWorker({ sharedMemory: false, withIngressToken: true }, async (f) => {
    f.send({ type: "create", setup: {}, seed: 1n });
    await Promise.resolve();
    // 非共享 WebAssembly.Memory 不可结构化克隆：ingress 消息不得携带
    // module/memory/token（串行形态下共享 ingress 面不成立），且 create
    // 全流程无 failure——修复前此处收到 DataCloneError failure，起局即崩。
    assert.deepEqual(f.posted[0], { type: "ingress", generation: 1, shared: false });
    assert.equal(f.posted[1].type, "created");
    assert.equal(f.posted[1].generation, 1);
    assert.equal(f.posted[2].type, "baseline");
    assert.equal(f.posted.filter((message) => message.type === "failure").length, 0);
  });
});

test("串行 substrate 且 bindings 缺 ingress_token 仍可起局（降级不依赖 ingress 面）", { timeout: 10000 }, async () => {
  await withWorker({ sharedMemory: false, withIngressToken: false }, async (f) => {
    f.send({ type: "create", setup: {}, seed: 1n });
    await Promise.resolve();
    // 修复前：postIngressBridge 先查 ingress_token 直接抛"缺少共享 ingress"，
    // 串行形态下无共享 ingress 面可缺——降级消息不携带 token，起局照常完成。
    assert.deepEqual(f.posted[0], { type: "ingress", generation: 1, shared: false });
    assert.equal(f.posted[1].type, "created");
    assert.equal(f.posted[2].type, "baseline");
    assert.equal(f.posted.filter((message) => message.type === "failure").length, 0);
  });
});

test("非跨源隔离环境（SharedArrayBuffer 全局不暴露）探测不得抛 ReferenceError", { timeout: 10000 }, async () => {
  // 浏览器非隔离上下文不暴露 SharedArrayBuffer 全局：探测 substrate 若直接
  // `buffer instanceof SharedArrayBuffer`，求值本身抛
  // ReferenceError: SharedArrayBuffer is not defined（浏览器实测，见
  // agents/w3-bridge-fix 台账）——必须先确认标识符存在再 instanceof。
  const descriptor = Object.getOwnPropertyDescriptor(globalThis, "SharedArrayBuffer");
  assert.ok(descriptor, "Node 环境应存在 SharedArrayBuffer 全局");
  Reflect.deleteProperty(globalThis, "SharedArrayBuffer");
  try {
    await withWorker({ sharedMemory: false, withIngressToken: true }, async (f) => {
      f.send({ type: "create", setup: {}, seed: 1n });
      await Promise.resolve();
      // 修复前：探测抛 ReferenceError → create 失败（failure 面含
      // "SharedArrayBuffer is not defined"）；修复后：视为非共享，正常降级。
      assert.deepEqual(f.posted[0], { type: "ingress", generation: 1, shared: false });
      assert.equal(f.posted[1].type, "created");
      assert.equal(f.posted[2].type, "baseline");
      assert.equal(f.posted.filter((message) => message.type === "failure").length, 0);
    });
  } finally {
    Object.defineProperty(globalThis, "SharedArrayBuffer", descriptor);
  }
});

test("共享 substrate（真线程形态）仍回传 module+memory+token 且可结构化克隆", { timeout: 10000 }, async () => {
  await withWorker({ sharedMemory: true, withIngressToken: true }, async (f) => {
    f.send({ type: "create", setup: {}, seed: 1n });
    await Promise.resolve();
    const ingress = f.posted[0];
    assert.deepEqual(
      { ...ingress, module: undefined, memory: undefined },
      { type: "ingress", generation: 1, shared: true, token: 7, diagnostics: false, module: undefined, memory: undefined },
    );
    assert.ok(ingress.module instanceof WebAssembly.Module);
    assert.ok((ingress.memory as WebAssembly.Memory).buffer instanceof SharedArrayBuffer);
    // fixture 的 postMessage 已内置 structuredClone：消息入账即证明可克隆。
    assert.equal(f.posted[1].type, "created");
    assert.equal(f.posted[2].type, "baseline");
    assert.equal(f.posted.filter((message) => message.type === "failure").length, 0);
  });
});
