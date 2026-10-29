import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";

test("生产 Worker init 把原输入交唯一initializer，配置前create拒绝且ready晚于确认", { timeout: 10000 }, async () => {
  const key = Symbol.for("stock-game-worker-facade-init-test");
  const posted: Record<string, unknown>[] = [], initialized: unknown[] = [], created: unknown[] = [];
  let handler!: (event: { data: Record<string, unknown> }) => void;
  let release!: () => void, configured = false;
  const wait = new Promise<void>((resolve) => { release = resolve; });
  const setup = { marker: "same-setup" };
  const memory = new WebAssembly.Memory({ initial: 1, maximum: 2, shared: true });
  const substrate = { memory, module: new WebAssembly.Module(new Uint8Array([0,97,115,109,1,0,0,0])), diagnostics: false };
  const saved = new Map<PropertyKey, PropertyDescriptor | undefined>();
  function replace(name: PropertyKey, value: unknown) { saved.set(name, Object.getOwnPropertyDescriptor(globalThis, name)); Object.defineProperty(globalThis, name, { value, configurable: true }); }
  const channels: MessageChannel[] = [], OriginalChannel = MessageChannel;
  replace("MessageChannel", class extends OriginalChannel { constructor() { super(); channels.push(this); } });
  const state = { initialized, created, setup, substrate, load: async (): Promise<unknown> => { throw new Error("load 未接线"); }, async initialize(input: unknown) { initialized.push(input); await this.load(); await wait; configured = true; return substrate; }, requireConfigured() { if (!configured) throw new Error("fixture not configured"); return { setup, substrate, initialArchive: null }; }, dispose() {} };
  replace(key, state);
  replace("self", { crossOriginIsolated: true, postMessage(message: Record<string, unknown>) { posted.push(message); }, addEventListener(_type: string, callback: typeof handler) { handler = callback; } });
  replace("fetch", async () => ({ ok: true, arrayBuffer: async () => new Uint8Array([0,97,115,109,1,0,0,0]).buffer }));
  const hooks = registerHooks({
    resolve(specifier, context, next) {
      if (specifier === "./facade-engine-initialize.ts" || specifier === "../../wasm-pkg/web_wasm.js") return { url: `virtual:facade-init:${specifier}`, shortCircuit: true };
      return next(specifier, context);
    },
    load(url, context, next) {
      if (url === "virtual:facade-init:./facade-engine-initialize.ts") return { format: "module", shortCircuit: true, source: 'export function createFacadeEngineInitializer(options) { const f=globalThis[Symbol.for("stock-game-worker-facade-init-test")]; f.load=options.load; return f; }' };
      if (url === "virtual:facade-init:../../wasm-pkg/web_wasm.js") return { format: "module", shortCircuit: true, source: 'const f=globalThis[Symbol.for("stock-game-worker-facade-init-test")]; export default async()=>f.substrate; export function create_session(setup){f.created.push(setup);return 1}; export function host_capabilities(){return {npcDecisionDiagnostics:false}}; export function ingress_token(){return 1}; export function snapshot(){return {}}; export function civil_date(){return "2030-01-02"}' };
      const loaded = next(url, context);
      if (url.includes("wasm-worker.ts?facade-init")) return { ...loaded, source: String(loaded.source).replaceAll("import.meta.env", '({DEV:false,MODE:"e2e"})') };
      return loaded;
    },
  });
  const channel = new MessageChannel();
  try {
    const workerUrl = "./wasm-worker.ts?facade-init";
    await import(workerUrl);
    const input = { type: "init", setup, controlPort: channel.port2 };
    handler({ data: input }); await new Promise((resolve) => setImmediate(resolve));
    assert.deepEqual(initialized, [input]); assert.equal(posted.some((item) => item.type === "ready"), false);
    handler({ data: { type: "create", setup: { marker: "wrong-second-copy" }, seed: 1n } });
    assert.equal(created.length, 0); assert.equal(posted.at(-1)?.type, "failure");
    posted.length = 0; release(); await new Promise((resolve) => setImmediate(resolve));
    assert.deepEqual(posted, [{ type: "ready" }]);
    handler({ data: { type: "create", seed: 1n } });
    assert.deepEqual(created, [setup]);
  } finally { release(); hooks.deregister(); for (const current of channels) { current.port1.close(); current.port2.close(); } for (const [name, descriptor] of saved) { if (descriptor === undefined) Reflect.deleteProperty(globalThis, name); else Object.defineProperty(globalThis, name, descriptor); } }
});
