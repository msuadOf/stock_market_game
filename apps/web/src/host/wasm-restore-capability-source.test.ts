import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";

for (const scenario of ["valid", "missing", "bad-provenance", "wrong-package", "missing-runtime", "obsolete-world-abi", "current-world-abi", "false-missing-abi", "false-with-abi"] as const) {
  test(`正式 restore metadata source ${scenario} 必须真实核字节与身份后才安装 owner source`, { timeout: 10000 }, async () => {
    const installs: { key: object; source: { forArchive(): unknown } }[] = [];
    const key = Symbol.for(`restore-metadata-source-${scenario}`);
    const original = Object.getOwnPropertyDescriptor(globalThis, key);
    Object.defineProperty(globalThis, key, { configurable: true, value: (documentKey: object, source: { forArchive(): unknown }) => { installs.push({ key: documentKey, source }); } });
    const hooks = registerHooks({
      resolve(specifier, context, next) {
        if (specifier === "./wasm-document-memory-owner.ts") return { url: `virtual:metadata-owner:${scenario}`, shortCircuit: true };
        return next(specifier, context);
      },
      load(url, context, next) {
        if (url.startsWith("virtual:metadata-owner:")) return { shortCircuit: true, format: "module", source: `export const configureDocumentWasmRestoreCapabilities = globalThis[Symbol.for(${JSON.stringify(`restore-metadata-source-${scenario}`)})];` };
        return next(url, context);
      },
    });
    const provenance = { kind: "wasm-build-provenance", sourceSha256: "c".repeat(64), wasmSha256: "a".repeat(64), glueSha256: "b".repeat(64), profile: "release", toolchain: "fixture-pin" };
    const provenanceText = JSON.stringify(scenario === "wrong-package" ? { ...provenance, sourceSha256: "e".repeat(64) } : provenance, null, 2) + "\n";
    const bytes = new TextEncoder().encode(provenanceText);
    const manifestHash = [...new Uint8Array(await crypto.subtle.digest("SHA-256", bytes))].map((byte) => byte.toString(16).padStart(2, "0")).join("");
    const world = scenario === "obsolete-world-abi" || scenario === "current-world-abi";
    const runtimeExports = world
      ? [scenario === "current-world-abi" ? "world_runtime_abi_version" : "set_step_runtime_override",
          "init_wasm_wait", "wasm_wait_abort_layout", "wasm_wait_death_layout", "shutdown_session"]
      : scenario === "false-with-abi" ? ["world_runtime_abi_version"] : [];
    const capability = { kind: "verified-package", wasmSha256: provenance.wasmSha256, glueSha256: provenance.glueSha256, sourceSha256: provenance.sourceSha256,
      buildManifestSha256: scenario === "bad-provenance" ? "f".repeat(64) : manifestHash, exports: ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore", "wasm_tombstone_init_primary", "wasm_tombstone_bind_ingress", "start_wasm_reclaimer", "close_wasm_reclaimer", ...runtimeExports] };
    const urls: string[] = [];
    const read = async (input: string | URL | Request): Promise<Response> => {
      const url = String(input); urls.push(url);
      if (scenario === "missing") return new Response("missing", { status: 404 });
      return new Response(url.endsWith("verified-restore-capability.json") ? JSON.stringify(capability) : provenanceText, { status: 200 });
    };
    try {
      const { prepareDocumentWasmRestoreCapability } = await import(`./wasm-restore-capability-source.ts?test=${scenario}`);
      const documentKey = {};
      const pending = prepareDocumentWasmRestoreCapability(documentKey, { baseUrl: "/game/", fetch: read as typeof fetch, ...(world || scenario === "missing-runtime" ? { runtimeOverride: true } : scenario === "false-missing-abi" || scenario === "false-with-abi" ? { runtimeOverride: false } : {}) });
      if (scenario === "valid" || scenario === "current-world-abi" || scenario === "false-with-abi") {
        await pending;
        await prepareDocumentWasmRestoreCapability(documentKey, { baseUrl: "/game/", fetch: read as typeof fetch });
        assert.equal(installs.length, 1); assert.equal(installs[0].key, documentKey); assert.deepEqual(installs[0].source.forArchive(), capability);
        assert.deepEqual(urls.sort(), ["/game/verified-restore-capability.json", "/game/wasm-build-provenance.json"]);
      } else {
        await assert.rejects(pending);
        assert.equal(installs.length, 0, "INVALID_METADATA_MUST_NOT_INSTALL_OWNER_SOURCE");
      }
    } finally { hooks.deregister(); if (original === undefined) Reflect.deleteProperty(globalThis, key); else Object.defineProperty(globalThis, key, original); }
  });
}
