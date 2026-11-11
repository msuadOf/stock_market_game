import { configureDocumentWasmRestoreCapabilities } from "./wasm-document-memory-owner.ts";
import { parseVerifiedRestoreCapability } from "./wasm-restore-package-capability.ts";
import type { VerifiedWasmRestoreCapability } from "./wasm-document-memory-owner.ts";
import type { WasmRestoreMode } from "./wasm-restore-package-capability.ts";

const preparations = new WeakMap<object, Promise<VerifiedWasmRestoreCapability>>();

/** 正式 index 与验收入口共用实际 build asset；新局不调用本准备器。 */
export async function prepareDocumentWasmRestoreCapability(documentKey: object, options: {
  readonly baseUrl?: string;
  readonly fetch?: typeof fetch;
  readonly runtimeOverride?: boolean;
  readonly restoreMode?: WasmRestoreMode;
} = {}): Promise<void> {
  let pending = preparations.get(documentKey);
  if (pending === undefined) {
    const base = options.baseUrl === undefined ? import.meta.env.BASE_URL : options.baseUrl;
    const read = options.fetch === undefined ? globalThis.fetch.bind(globalThis) : options.fetch;
    pending = (async () => {
      const [capResponse, provenanceResponse] = await Promise.all([read(`${base}verified-restore-capability.json`), read(`${base}wasm-build-provenance.json`)]);
      if (!capResponse.ok || !provenanceResponse.ok) throw new Error(`实际 WASM restore metadata 加载失败：cap HTTP ${capResponse.status}，provenance HTTP ${provenanceResponse.status}；尚未准入或分配 Memory`);
      const [capValue, provenanceBytes] = await Promise.all([capResponse.json() as Promise<unknown>, provenanceResponse.arrayBuffer()]);
      const capability = parseVerifiedRestoreCapability(capValue, options.runtimeOverride, options.restoreMode);
      const digest = await crypto.subtle.digest("SHA-256", provenanceBytes);
      const manifestHash = [...new Uint8Array(digest)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
      if (manifestHash !== capability.buildManifestSha256) throw new Error("实际 build provenance 字节 SHA-256 与 restore capability 不符；未准入 Memory");
      const provenance: unknown = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(provenanceBytes));
      if (provenance === null || typeof provenance !== "object" || Array.isArray(provenance)) throw new Error("build provenance 必须为对象");
      const source = provenance as Record<string, unknown>;
      if (Object.keys(source).sort().join(",") !== "glueSha256,kind,profile,sourceSha256,toolchain,wasmSha256" || source.kind !== "wasm-build-provenance" || source.profile !== "release"
        || typeof source.toolchain !== "string" || source.toolchain.trim().length === 0
        || source.sourceSha256 !== capability.sourceSha256 || source.wasmSha256 !== capability.wasmSha256 || source.glueSha256 !== capability.glueSha256) throw new Error("build provenance 来源、profile 或 package 身份与 capability 不符；未准入 Memory");
      configureDocumentWasmRestoreCapabilities(documentKey, {
        forArchive: () => capability,
        forLease: () => parseVerifiedRestoreCapability(capability, options.runtimeOverride, "chunked"),
      });
      return capability;
    })().catch((error: unknown) => { preparations.delete(documentKey); throw error; });
    preparations.set(documentKey, pending);
  }
  parseVerifiedRestoreCapability(await pending, options.runtimeOverride, options.restoreMode);
}
