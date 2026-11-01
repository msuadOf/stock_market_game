import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

const hash = (value) => createHash("sha256").update(value).digest("hex");
const required = ["restore_json", "facade_slot_init_for_restore_json", "restore_prepared", "clear_prepared_restore", "set_step_runtime_override", "init_wasm_wait", "wasm_wait_abort_layout", "shutdown_session"];
const provenanceBytes = Buffer.from(`${JSON.stringify({ kind: "wasm-build-provenance", sourceSha256: "a".repeat(64), wasmSha256: "b".repeat(64), glueSha256: "c".repeat(64), profile: "release", toolchain: "nightly-2026-09-05" }, null, 2)}\n`);
const capabilityBytes = Buffer.from(`${JSON.stringify({ kind: "verified-package", wasmSha256: "b".repeat(64), glueSha256: "c".repeat(64), sourceSha256: "a".repeat(64), buildManifestSha256: hash(provenanceBytes), exports: [...required].sort() }, null, 2)}\n`);
const { wasmPackageCapabilityPlugin } = await import("./wasm-package-capability.ts");

async function fixture(run) {
  const root = await mkdtemp(path.join(os.tmpdir(), "wasm-vite-metadata-"));
  const pkg = path.join(root, "wasm-pkg");
  await mkdir(pkg);
  await writeFile(path.join(pkg, "wasm-build-provenance.json"), provenanceBytes);
  await writeFile(path.join(pkg, "verified-restore-capability.json"), capabilityBytes);
  try { await run(root, pkg); } finally { await rm(root, { recursive: true, force: true }); }
}

test("Vite build emits only the validated metadata bytes at the BASE root", { timeout: 10000 }, async () => fixture(async (root) => {
  const plugin = wasmPackageCapabilityPlugin();
  plugin.configResolved({ root, base: '/' });
  const emitted = [];
  await plugin.generateBundle.call({ emitFile: (asset) => emitted.push(asset) });
  assert.deepEqual(emitted.map(({ fileName }) => fileName).sort(), ["verified-restore-capability.json", "wasm-build-provenance.json"]);
  assert.deepEqual(emitted.find(({ fileName }) => fileName === "wasm-build-provenance.json").source, provenanceBytes);
  assert.deepEqual(emitted.find(({ fileName }) => fileName === "verified-restore-capability.json").source, capabilityBytes);
}));

test("Vite dev serves only the two validated metadata files and passes unrelated URLs onward", { timeout: 10000 }, async () => fixture(async (root) => {
  const plugin = wasmPackageCapabilityPlugin();
  plugin.configResolved({ root, base: '/' });
  let middleware;
  plugin.configureServer({ middlewares: { use: (handler) => { middleware = handler; } } });
  const send = (url) => new Promise((resolve, reject) => {
    const response = { headers: {}, setHeader(name, value) { this.headers[name] = value; }, end(value) { resolve({ statusCode: this.statusCode, headers: this.headers, body: Buffer.from(value) }); } };
    middleware({ url }, response, (error) => error === undefined ? resolve({ next: true }) : reject(error));
  });
  assert.deepEqual((await send("/wasm-build-provenance.json?cache=0")).body, provenanceBytes);
  assert.deepEqual(await send("/unrelated.json"), { next: true });
  assert.deepEqual(await send("/nested/wasm-build-provenance.json"), { next: true });
}));

test("Vite dev reports malformed metadata instead of serving it", { timeout: 10000 }, async () => fixture(async (root, pkg) => {
  await writeFile(path.join(pkg, "verified-restore-capability.json"), "{}\n");
  const plugin = wasmPackageCapabilityPlugin();
  plugin.configResolved({ root, base: '/' });
  let middleware;
  plugin.configureServer({ middlewares: { use: (handler) => { middleware = handler; } } });
  await assert.rejects(new Promise((resolve, reject) => middleware({ url: "/verified-restore-capability.json" }, { setHeader() {}, end: resolve }, reject)), /schema mismatch|identities/);
}));
