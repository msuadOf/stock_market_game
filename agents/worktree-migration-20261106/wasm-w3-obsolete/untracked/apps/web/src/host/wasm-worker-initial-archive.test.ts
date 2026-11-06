import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";
import { catalogWorkerFixture } from "./catalog-test-fixture.ts";
import { snapshot } from "./protocol-test-fixtures.ts";
import { commandDayEndArchiveFixture } from "../app/command-host-test-fixture.ts";

type Message = Record<string, unknown> & { type: string };
const key = Symbol.for("stock-game-initial-archive-worker");
let runId = 0;
async function fixture(run: (f: { send(message: Message): void; calls: string[]; posted: Message[]; fail(): void; archive: unknown }) => Promise<void> | void, restored = true) {
  const saved = new Map<PropertyKey, PropertyDescriptor | undefined>();
  function replace(name: PropertyKey, value: unknown) { saved.set(name, Object.getOwnPropertyDescriptor(globalThis, name)); Object.defineProperty(globalThis, name, { configurable: true, value }); }
  const channels: MessageChannel[] = [], OriginalChannel = MessageChannel;
  replace("MessageChannel", class extends OriginalChannel { constructor() { super(); channels.push(this); } });
  const calls: string[] = [], posted: Message[] = []; let handler!: (event: { data: Message }) => void; let failing = false;
  let initialized!: () => void;
  const ready = new Promise<void>((resolve) => { initialized = resolve; });
  const archive = commandDayEndArchiveFixture(); const catalog = catalogWorkerFixture(snapshot(0, 0));
  const state = {
    calls, configured: false, archive: restored ? archive : null,
    bindings: {
      create_session: () => { calls.push("create"); if (failing) throw new Error("首建失败"); return 11; },
      restore_json: (text: string) => { calls.push("restore"); assert.deepEqual(JSON.parse(text), archive); if (failing) throw new Error("首建失败"); return 11; },
      prepare_public_baseline: () => { calls.push("prepare"); }, snapshot: () => snapshot(0, 0),
      stock_catalog_view: () => catalog.stockCatalog, company_catalog_view: () => catalog.companyCatalog,
      civil_date: () => "2030-01-02", ingress_token: () => 11,
      host_capabilities: () => ({ npcDecisionDiagnostics: false, indicatorCapabilities: { intradayAverage: false, macd: false, priceKdj: false, candleKdj: false }, personalTradeHistory: false }),
      drop_session: () => { calls.push("drop"); },
    },
  };
  replace(key, state); replace("self", { crossOriginIsolated: true, postMessage: (value: Message) => { posted.push(value); if (value.type === "ready" || value.type === "failure") initialized(); }, addEventListener: (_type: string, callback: typeof handler) => { handler = callback; } });
  replace("fetch", async () => ({ ok: true, arrayBuffer: async () => new Uint8Array([0,97,115,109,1,0,0,0]).buffer }));
  const id = ++runId;
  const hooks = registerHooks({
    resolve(specifier, context, next) {
      if (specifier === "./facade-engine-initialize.ts" || specifier === "../../wasm-pkg/web_wasm.js") return { url: `virtual:initial-archive:${id}:${specifier}`, shortCircuit: true };
      return next(specifier, context);
    },
    load(url, context, next) {
      if (url.startsWith(`virtual:initial-archive:${id}:`)) {
        const prefix = 'const f=globalThis[Symbol.for("stock-game-initial-archive-worker")];';
        return { shortCircuit: true, format: "module", source: url.endsWith("facade-engine-initialize.ts")
          ? `${prefix} export function createFacadeEngineInitializer(options) { return { async initialize(){await options.load();f.configured=true;}, requireConfigured(){if(!f.configured)throw new Error("not configured");return {setup:f.archive===null?{}:f.archive.setup,initialArchive:f.archive};},dispose(){} }; }`
          : prefix + Object.keys(state.bindings).map((name) => `export const ${name}=(...args)=>f.bindings.${name}(...args);`).join("\n") };
      }
      const loaded = next(url, context);
      return url.includes("wasm-worker.ts?initial-archive=") ? { ...loaded, source: String(loaded.source).replaceAll("import.meta.env", '({DEV:false,MODE:"e2e"})') } : loaded;
    },
  });
  try {
    await import(`./wasm-worker.ts?initial-archive=${id}`);
    handler({ data: { type: "init" } }); await ready;
    assert.deepEqual(posted, [{ type: "ready" }]); posted.length = 0;
    await run({ send: (message) => handler({ data: message }), calls, posted, fail: () => { failing = true; }, archive });
  } finally { for (const channel of channels) { channel.port1.close(); channel.port2.close(); } hooks.deregister(); for (const [name, descriptor] of saved) { if (descriptor === undefined) Reflect.deleteProperty(globalThis, name); else Object.defineProperty(globalThis, name, descriptor); } }
}

test("生产Worker初次档只restore一个World，ingress→created→baseline后才交付首代", async () => {
  await fixture((f) => {
    f.send({ type: "restoreInitial" });
    assert.equal(f.calls.filter((call) => call === "restore").length, 1); assert.equal(f.calls.includes("create"), false);
    assert.deepEqual(f.posted.map((message) => message.type), ["ingress", "created", "baseline"]);
    assert.ok(f.posted.every((message) => message.generation === 1));
  });
});
for (const extra of ["setup", "initialArchive", "seed"]) {
  test(`restoreInitial拒绝夹带第二份${extra}，不调用FFI`, async () => {
    await fixture((f) => { f.send({ type: "restoreInitial", [extra]: {} }); assert.deepEqual(f.calls, []); assert.equal(f.posted.at(-1)?.type, "failure"); assert.match(String(f.posted.at(-1)?.message), /只接受 type/); f.send({ type: "restoreInitial" }); assert.deepEqual(f.calls, []); });
  });
}
test("选档实例拒绝create，普通新局实例拒绝restoreInitial", async () => {
  await fixture((f) => { f.send({ type: "create", seed: 1n }); assert.deepEqual(f.calls, []); assert.equal(f.posted.at(-1)?.type, "failure"); });
  await fixture((f) => { f.send({ type: "restoreInitial" }); assert.deepEqual(f.calls, []); assert.equal(f.posted.at(-1)?.type, "failure"); }, false);
});
for (const restored of [true, false]) {
  test(`${restored ? "restore" : "create"}首建失败后同实例再次首建不再调用FFI`, async () => {
    await fixture((f) => {
      f.fail(); const message = restored ? { type: "restoreInitial" } : { type: "create", seed: 1n };
      f.send(message); f.send(message);
      assert.deepEqual(f.calls, [restored ? "restore" : "create"]);
      assert.equal(f.posted.filter((item) => item.type === "failure").length, 2);
    }, restored);
  });
}
test("成功首建后重复或跨模式首建均拒绝，旧World不被覆盖", async () => {
  await fixture((f) => {
    f.send({ type: "restoreInitial" }); const before = [...f.calls];
    f.send({ type: "restoreInitial" }); f.send({ type: "create", seed: 1n });
    assert.deepEqual(f.calls, before); assert.equal(f.posted.filter((item) => item.type === "created").length, 1);
  });
});
