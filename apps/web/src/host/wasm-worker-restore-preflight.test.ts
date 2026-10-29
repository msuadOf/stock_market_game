import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";

for (const scenario of ["wrong-hash", "missing-export", "false-missing-setter"] as const) {
  test(`实际 Worker restore package ${scenario} 必须在 initializer/Memory 前拒绝`, { timeout: 10000 }, async () => {
    const key = Symbol.for(`restore-preflight-${scenario}`);
    const saved = new Map<PropertyKey, PropertyDescriptor | undefined>();
    function replace(name: PropertyKey, value: unknown) { saved.set(name, Object.getOwnPropertyDescriptor(globalThis, name)); Object.defineProperty(globalThis, name, { value, configurable: true }); }
    const channels: MessageChannel[] = [];
    const OriginalChannel = MessageChannel;
    replace("MessageChannel", class extends OriginalChannel { constructor() { super(); channels.push(this); } });
    const posted: Record<string, unknown>[] = [];
    let calls = 0; let handler!: (event: { data: Record<string, unknown> }) => void;
    replace(key, { initialize() { calls++; return Promise.resolve(); }, dispose() {} });
    replace("self", { postMessage: (message: Record<string, unknown>) => { posted.push(message); }, addEventListener(_type: string, listener: typeof handler) { handler = listener; } });
    const bytes = new Uint8Array([0,97,115,109,1,0,0,0]);
    const wasmHash = [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((byte) => byte.toString(16).padStart(2, "0")).join("");
    let fetches = 0;
    replace("fetch", async () => { fetches++; return { ok: true, arrayBuffer: async () => bytes.buffer }; });
    const hooks = registerHooks({
      resolve(specifier, context, next) {
        if (specifier === "./facade-engine-initialize.ts") return { url: `virtual:restore-preflight-init:${scenario}`, shortCircuit: true };
        if (specifier === "../../wasm-pkg/web_wasm.js") return { url: `virtual:restore-preflight-glue:${scenario}`, shortCircuit: true };
        return next(specifier, context);
      },
      load(url, context, next) {
        if (url.startsWith("virtual:restore-preflight-init:")) return { format: "module", shortCircuit: true, source: `export function createFacadeEngineInitializer(){return globalThis[Symbol.for(${JSON.stringify(`restore-preflight-${scenario}`)})]}` };
        if (url.startsWith("virtual:restore-preflight-glue:")) return { format: "module", shortCircuit: true, source: "export function restore_json(){}; export function facade_slot_init_for_restore_json(){};" };
        const loaded = next(url, context);
        if (url.includes("wasm-worker.ts?restore-preflight=")) return { ...loaded, source: String(loaded.source).replaceAll("import.meta.env", '({DEV:false,MODE:"e2e"})') };
        return loaded;
      },
    });
    try {
      await import(`./wasm-worker.ts?restore-preflight=${scenario}`);
      handler({ data: { type: "init", ...(scenario === "false-missing-setter" ? { runtimeOverride: false } : {}), initialArchive: { marker: "唯一 X" }, restoreCapability: { kind: "verified-package", wasmSha256: scenario === "wrong-hash" ? "a".repeat(64) : wasmHash,
        glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: ["restore_json", "facade_slot_init_for_restore_json"] } } });
      for (let count = 0; count < 20 && posted.length === 0; count++) await new Promise((resolve) => setImmediate(resolve));
      if (scenario === "false-missing-setter") assert.equal(fetches, 0, "缺 setter 在 package load 和 initializer 前拒绝");
      assert.equal(calls, 0, "PACKAGE_CHECK_MUST_PRECEDE_MEMORY_INITIALIZER");
      assert.equal(posted.length, 1); assert.equal(posted[0].type, "failure");
      assert.equal(posted[0].engineQuiescent, "initialization-failed"); assert.equal(posted[0].memoryAllocation, "not-allocated");
      assert.equal(posted[0].code, scenario === "wrong-hash" ? "WASM_PACKAGE_IDENTITY_MISMATCH" : "WASM_RESTORE_TOPOLOGY_NOT_READY");
    } finally { hooks.deregister(); for (const channel of channels) { channel.port1.close(); channel.port2.close(); } for (const [name, value] of saved) { if (value === undefined) Reflect.deleteProperty(globalThis, name); else Object.defineProperty(globalThis, name, value); } }
  });
}
