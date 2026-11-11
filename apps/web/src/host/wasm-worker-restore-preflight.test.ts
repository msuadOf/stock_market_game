import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";
import { WorkerTransportFixture } from "./worker-transport.test-support.ts";

for (const scenario of ["wrong-hash", "missing-export", "false-missing-setter", "chunk-missing"] as const) {
  test(`实际 Worker restore package ${scenario} 必须在 initializer/Memory 前拒绝`, { timeout: 10000 }, async (t) => {
    const key = Symbol.for(`restore-preflight-${scenario}`);
    const saved = new Map<PropertyKey, PropertyDescriptor | undefined>();
    function replace(name: PropertyKey, value: unknown) { saved.set(name, Object.getOwnPropertyDescriptor(globalThis, name)); Object.defineProperty(globalThis, name, { value, configurable: true }); }
    const transport = new WorkerTransportFixture();
    const posted: Record<string, unknown>[] = [];
    let releaseCompile: (() => void) | undefined;
    let resolveCompileStarted!: () => void;
    const compileStarted = new Promise<void>((resolve) => { resolveCompileStarted = resolve; });
    const compileGate = new Promise<void>((resolve) => { releaseCompile = resolve; });
    let resolvePosted!: (message: Record<string, unknown>) => void;
    const postedMessage = new Promise<Record<string, unknown>>((resolve) => { resolvePosted = resolve; });
    function awaitTestSignal<T>(promise: Promise<T>): Promise<T> {
      return new Promise<T>((resolve, reject) => {
        if (t.signal.aborted) { reject(t.signal.reason); return; }
        const onAbort = () => reject(t.signal.reason);
        t.signal.addEventListener("abort", onAbort, { once: true });
        promise.then(
          (value) => { t.signal.removeEventListener("abort", onAbort); resolve(value); },
          (error: unknown) => { t.signal.removeEventListener("abort", onAbort); reject(error); },
        );
      });
    }
    let calls = 0; let handler!: (event: { data: Record<string, unknown> }) => void;
    replace(key, { initialize() { calls++; return Promise.resolve(); }, dispose() {}, async disposeAsync() {} });
    const lifecycle: Record<string, unknown>[] = [];
    replace("self", { postMessage: (message: Record<string, unknown>) => { lifecycle.push(message); }, reportError(error: unknown) { throw error; }, addEventListener(_type: string, listener: typeof handler) { handler = listener; } });
    const bytes = new Uint8Array([0,97,115,109,1,0,0,0]);
    if (scenario === "missing-export") {
      const originalWebAssembly = WebAssembly;
      replace("WebAssembly", new Proxy(originalWebAssembly, {
        get(target, property) {
          if (property === "compile") return async (source: BufferSource) => {
            resolveCompileStarted();
            await compileGate;
            return Reflect.apply(target.compile, target, [source]);
          };
          return Reflect.get(target, property, target);
        },
      }));
    }
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
      if (handler === undefined) throw new Error("WASM Worker bootstrap listener 未安装");
      transport.bindEngineTransport(handler as (event: MessageEvent<unknown>) => void, 1);
      const workerPort = transport.channels[0]!.port2 as unknown as MessagePort;
      const ownerBusinessPort = transport.channels[0]!.port1 as unknown as MessagePort;
      const originalPost = workerPort.postMessage.bind(workerPort);
      workerPort.postMessage = (message: unknown) => { const value = message as Record<string, unknown>; posted.push(value); resolvePosted(value); originalPost(message); };
      transport.dispatch(ownerBusinessPort, { type: "init", ...(scenario === "false-missing-setter" ? { runtimeOverride: false } : {}), ...(scenario === "chunk-missing" ? { initialChunked: {} } : { initialArchive: { marker: "唯一 X" } }), restoreCapability: { kind: "verified-package", wasmSha256: scenario === "wrong-hash" ? "a".repeat(64) : wasmHash,
        glueSha256: "b".repeat(64), sourceSha256: "c".repeat(64), buildManifestSha256: "d".repeat(64), exports: ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore", "wasm_tombstone_init_primary", "wasm_tombstone_bind_ingress"] } });
      if (scenario === "missing-export") {
        const progress = await awaitTestSignal(Promise.race([
          compileStarted.then(() => "compile" as const),
          postedMessage.then(() => "failure" as const),
        ]));
        if (progress === "compile") {
          for (let count = 0; count < 20 && posted.length === 0; count++) await new Promise((resolve) => setImmediate(resolve));
          assert.equal(posted.length, 0, "20 次 setImmediate 不能替代真实 WebAssembly.compile 完成");
          releaseCompile?.();
          releaseCompile = undefined;
        }
      }
      if (scenario === "false-missing-setter" || scenario === "chunk-missing") assert.equal(fetches, 0, "缺能力在 package load 和 initializer 前拒绝");
      const failure = await awaitTestSignal(postedMessage);
      assert.equal(calls, 0, "PACKAGE_CHECK_MUST_PRECEDE_MEMORY_INITIALIZER");
      assert.equal(posted.length, 1); assert.equal(failure.type, "failure");
      assert.equal(failure.engineQuiescent, "initialization-failed"); assert.equal(failure.memoryAllocation, "not-allocated");
      assert.equal(failure.code, scenario === "wrong-hash" ? "WASM_PACKAGE_IDENTITY_MISMATCH" : "WASM_RESTORE_TOPOLOGY_NOT_READY");
    } finally { releaseCompile?.(); transport.close(); hooks.deregister(); for (const [name, value] of saved) { if (value === undefined) Reflect.deleteProperty(globalThis, name); else Object.defineProperty(globalThis, name, value); } }
  });
}
