import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";
import { currentSaveFixture } from "../save/current-save-fixture.ts";
import { civilUpdate, frame, snapshot, tickBatch } from "./protocol-test-fixtures.ts";

type Message = Record<string, unknown> & { type: string };
let fixtureId = 0;
const bindingsKey = Symbol.for("stock-game-wasm-owner-tests");

async function withWorker(run: (fixture: Awaited<ReturnType<typeof createFixture>>) => Promise<void>) {
  const globals = new Map<PropertyKey, PropertyDescriptor | undefined>();
  const replace = (key: PropertyKey, value: unknown) => {
    globals.set(key, Object.getOwnPropertyDescriptor(globalThis, key));
    Object.defineProperty(globalThis, key, { configurable: true, value });
  };
  let hooks: ReturnType<typeof registerHooks> | undefined;
  try {
    const f = await createFixture(replace, (registered) => { hooks = registered; });
    await run(f);
  } finally {
    hooks?.deregister();
    for (const [key, original] of globals) {
      if (original === undefined) Reflect.deleteProperty(globalThis, key);
      else Object.defineProperty(globalThis, key, original);
    }
  }
}

async function createFixture(
  replace: (key: PropertyKey, value: unknown) => void,
  register: (hooks: ReturnType<typeof registerHooks>) => void,
) {
  const posted: Message[] = [];
  let initialized: () => void = () => { throw new Error("初始化信号未安装"); };
  const ready = new Promise<void>((resolve) => { initialized = resolve; });
  const calls: string[] = [];
  const timers = new Map<number, () => void>();
  let nextTimer = 0;
  let handle = 7;
  let failing: "snapshot" | "prepare" | "drop" | null = null;
  let listener: ((event: { data: Message }) => void) | undefined;
  const saved = currentSaveFixture();
  const bindings = {
    default: async () => ({ memory: new WebAssembly.Memory({ initial: 1, maximum: 1, shared: true }) }),
    ingress_token: (current: number) => current,
    initThreadPool: async (threads: number) => { calls.push(`threads:${threads}`); },
    create_session: () => { calls.push(`create:${handle}`); return handle; },
    civil_date: (current: number) => current === 7 ? "2030-01-02" : "2031-02-03",
    snapshot: (current: number) => { calls.push(`snapshot:${current}`); if (failing === "snapshot") throw new Error("snapshot failed"); return { handle: current }; },
    prepare_public_baseline: (current: number) => { calls.push(`prepare:${current}`); if (failing === "prepare") throw new Error("prepare failed"); },
    drop_session: (current: number) => { calls.push(`drop:${current}`); if (failing === "drop") throw new Error("drop failed"); },
    restore_json: (encoded: string) => { assert.deepEqual(JSON.parse(encoded), saved); calls.push(`restore:${handle}`); return handle; },
    host_capabilities: () => ({ npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true }),
    step: (current: number) => { calls.push(`step:${current}`); return tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1)); },
    tick: () => 1n,
    save: (current: number) => { calls.push(`save:${current}`); return saved; },
    end_civil_day: () => civilUpdate(),
  };
  replace(bindingsKey, bindings);
  replace("self", { postMessage: (message: Message) => { posted.push(message); if (message.type === "ready" || message.type === "failure") initialized(); }, addEventListener: (_type: string, callback: typeof listener) => { listener = callback; } });
  replace("navigator", { hardwareConcurrency: 4 });
  replace("fetch", async () => ({ ok: true, arrayBuffer: async () => Uint8Array.from([0, 97, 115, 109, 1, 0, 0, 0]).buffer }));
  replace("setTimeout", (callback: () => void) => { const id = ++nextTimer; timers.set(id, callback); return id; });
  replace("clearTimeout", (id: number) => { timers.delete(id); });
  const id = ++fixtureId;
  register(registerHooks({
    resolve(specifier, context, nextResolve) {
      const resolved = nextResolve(specifier, context);
      if (resolved.url.endsWith("/wasm-pkg/web_wasm.js")) return { ...resolved, url: `${resolved.url}?owner-test=${id}` };
      return resolved;
    },
    load(url, context, nextLoad) {
      if (url.includes("/wasm-pkg/web_wasm.js?owner-test=")) {
        const exports = Object.keys(bindings).filter((key) => key !== "default")
          .map((key) => `export const ${key} = (...args) => bindings.${key}(...args);`).join("\n");
        return { shortCircuit: true, format: "module", source: `const bindings = globalThis[Symbol.for("stock-game-wasm-owner-tests")];\n${exports}\nexport default bindings.default;` };
      }
      const loaded = nextLoad(url, context);
      if (url.includes("/wasm-worker.ts?owner-test=")) {
        // 仅替换 Node 缺少的 Vite import.meta.env；协议 listener 与 owner 使用原源码。
        return { ...loaded, source: String(loaded.source).replaceAll("import.meta.env", '({ MODE: "e2e", DEV: false })') };
      }
      return loaded;
    },
  }));
  await import(`./wasm-worker.ts?owner-test=${id}`);
  assert.ok(listener);
  const send = (message: Message) => { listener!({ data: message }); };
  send({ type: "init", threads: 2 });
  await ready;
  assert.deepEqual(posted, [{ type: "ready", threads: 2 }]);
  posted.length = 0;
  return { send, calls, posted: posted as Message[], timers, saved, handle: (next: number) => { handle = next; }, fail: (next: typeof failing) => { failing = next; } };
}

test("Worker 全部会话 caller 共用 owner，restore 旧 generation 回应先于新 baseline 和 microtask restart", { timeout: 10000 }, async () => {
  await withWorker(async (f) => {
    f.send({ type: "create", setup: {}, seed: 1n });
    assert.equal(f.posted[0].type, "ingress");
    assert.equal(f.posted[0].token, 7);
    assert.equal(f.posted[0].generation, 1);
    assert.ok(f.posted[0].module instanceof WebAssembly.Module);
    assert.ok((f.posted[0].memory as WebAssembly.Memory).buffer instanceof SharedArrayBuffer);
    assert.deepEqual(f.posted.slice(1), [
      { type: "created", generation: 1, capabilities: { npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, personalTradeHistory: true } },
      { type: "baseline", generation: 1, snapshot: { handle: 7 }, civilDate: "2030-01-02" },
    ]);
    f.posted.length = 0;
    f.send({ type: "save", generation: 1, requestId: 1 });
    assert.deepEqual(f.posted, [{ type: "saved", requestId: 1, generation: 1, slot: f.saved }]);
    f.send({ type: "start", generation: 1, requestId: 20 });
    assert.deepEqual(f.posted.at(-1), { type: "started", generation: 1, requestId: 20 });
    assert.equal(f.timers.size, 1);
    f.posted.length = 0;
    f.handle(9);
    f.send({ type: "restore", generation: 1, requestId: 2, slot: f.saved });
    assert.equal(f.timers.size, 0, "restore 完成回应之前不能同步 restart");
    assert.equal(f.posted[0].type, "ingress");
    assert.equal((f.posted[0] as Message).token, 9);
    assert.equal(f.posted[0].generation, 2);
    assert.deepEqual(f.posted.slice(1), [
      { type: "restored", generation: 1, nextGeneration: 2, requestId: 2, snapshot: { handle: 9 }, civilDate: "2031-02-03" },
      { type: "baseline", generation: 2, snapshot: { handle: 9 }, civilDate: "2031-02-03" },
    ]);
    await Promise.resolve();
    assert.equal(f.timers.size, 1);
    f.posted.length = 0;
    f.send({ type: "save", generation: 1, requestId: 3 });
    assert.deepEqual(f.posted, [{ type: "operationError", requestId: 3, generation: 1, message: "Worker 请求属于已过期会话" }]);
    f.send({ type: "stop", generation: 2, requestId: 21 });
    assert.deepEqual(f.posted.at(-1), { type: "stopped", generation: 2, requestId: 21 });
    f.send({ type: "stepOnce", generation: 2, requestId: 4 });
    assert.ok(f.calls.includes("step:9"));
    assert.deepEqual(f.posted.at(-1), { type: "stepped", generation: 2, requestId: 4, tick: 1 });
    f.posted.length = 0;
    f.send({ type: "setPausePreferences", generation: 2, requestId: 5, preferences: { pause_after_close: true, pause_before_open: false } });
    f.send({ type: "start", generation: 2, requestId: 22 });
    f.send({ type: "endCivilDay", generation: 2, requestId: 6 });
    assert.deepEqual(f.posted.map((message) => message.type), ["pausePreferencesSet", "started", "protocol", "barrierPaused", "civilDayEnded"]);
    assert.equal(f.timers.size, 0);
    f.send({ type: "start", generation: 2, requestId: 23 });
    f.send({ type: "drop" });
    assert.equal(f.timers.size, 0);
    assert.equal(f.calls.at(-1), "drop:9");
  });
});

test("Worker restore snapshot 失败仍以 microtask 恢复原会话，prepare 失败保留新 authority", { timeout: 10000 }, async () => {
  await withWorker(async (f) => {
    f.send({ type: "create", setup: {}, seed: 1n });
    f.send({ type: "start", generation: 1, requestId: 20 });
    assert.deepEqual(f.posted.at(-1), { type: "started", generation: 1, requestId: 20 });
    f.handle(9);
    f.fail("snapshot");
    f.posted.length = 0;
    f.send({ type: "restore", generation: 1, requestId: 10, slot: f.saved });
    assert.equal(f.timers.size, 0);
    assert.deepEqual(f.posted, [{ type: "operationError", requestId: 10, generation: 1, message: "snapshot failed" }]);
    assert.equal(f.calls.at(-1), "drop:9");
    await Promise.resolve();
    assert.equal(f.timers.size, 1);
    f.fail("prepare");
    f.posted.length = 0;
    f.send({ type: "restore", generation: 1, requestId: 11, slot: f.saved });
    assert.deepEqual(f.posted, [{ type: "operationError", requestId: 11, generation: 1, message: "prepare failed" }]);
    await Promise.resolve();
    assert.equal(f.timers.size, 1);
    f.fail(null);
    f.send({ type: "refreshBaseline", generation: 2, requestId: 12 });
    assert.deepEqual(f.posted.at(-1), { type: "refreshed", generation: 2, requestId: 12, snapshot: { handle: 9 }, civilDate: "2031-02-03" });
    f.send({ type: "drop" });
    assert.equal(f.timers.size, 0);
  });
});
