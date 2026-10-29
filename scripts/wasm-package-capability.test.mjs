import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import {
  WASM_PACKAGE_METADATA_FILENAMES,
  parseWasmPackageMetadata,
  produceWasmPackageMetadata,
  captureWasmBuildSourceIdentity,
} from "./wasm-package-capability.mjs";

const hash = (value) => createHash("sha256").update(value).digest("hex");
const required = ["restore_json", "facade_slot_init_for_restore_json", "set_step_runtime_override", "init_wasm_wait", "wasm_wait_abort_layout", "shutdown_session"];
const wasm = Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]);
const glue = Buffer.from("export function restore_json() {}\n");
function bytesFor(provenance = {}) {
  const value = { kind: "wasm-build-provenance", sourceSha256: "a".repeat(64), wasmSha256: "b".repeat(64), glueSha256: "c".repeat(64), profile: "release", toolchain: "nightly-2026-09-05", ...provenance };
  const raw = Buffer.from(`${JSON.stringify(value, null, 2)}\n`);
  const capability = { kind: "verified-package", wasmSha256: value.wasmSha256, glueSha256: value.glueSha256, sourceSha256: value.sourceSha256, buildManifestSha256: hash(raw), exports: [...required].sort() };
  return [raw, Buffer.from(`${JSON.stringify(capability, null, 2)}\n`)];
}

test("metadata parser enforces the exact six-field cross-file identity contract", { timeout: 10000 }, () => {
  const [provenance, capability] = bytesFor();
  assert.equal(parseWasmPackageMetadata(provenance, capability).capability.kind, "verified-package");
  const wrong = bytesFor({ profile: "dev" });
  assert.throws(() => parseWasmPackageMetadata(...wrong), /schema or build identity/);
  const extra = Buffer.from(`${JSON.stringify({ ...JSON.parse(provenance), extra: true })}\n`);
  assert.throws(() => parseWasmPackageMetadata(extra, capability), /schema or build identity/);
  const driftValue = JSON.parse(capability);
  driftValue.exports = ["restore_json"];
  const drift = Buffer.from(`${JSON.stringify(driftValue, null, 2)}\n`);
  assert.throws(() => parseWasmPackageMetadata(provenance, drift), /missing required public functions/);
});

test("source identity is deterministic over the declared Rust compiler input closure", { timeout: 10000 }, async () => {
  const root = await mkdtemp(path.join(os.tmpdir(), "wasm-source-id-"));
  try {
    for (const relative of ["Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml", "apps/web-wasm/Cargo.toml", "apps/web-wasm/.cargo/config.toml", "apps/web-wasm/src/lib.rs", "packages/engine/Cargo.toml", "packages/engine/src/lib.rs"]) {
      const filename = path.join(root, relative);
      await mkdir(path.dirname(filename), { recursive: true });
      await writeFile(filename, relative);
    }
    const first = await captureWasmBuildSourceIdentity(root);
    const second = await captureWasmBuildSourceIdentity(root);
    assert.deepEqual(first, second);
    assert.deepEqual(first.map(({ path: item }) => item), first.map(({ path: item }) => item).sort());
  } finally { await rm(root, { recursive: true, force: true }); }
});

test("producer refuses non-pinned or non-release command identities", { timeout: 10000 }, async () => {
  await assert.rejects(produceWasmPackageMetadata({ root: path.resolve("."), outDir: path.resolve("."), command: { command: "wasm-pack", args: ["build", "--release"], env: {} }, sourceSnapshot: [{ path: "Cargo.lock", sha256: "a".repeat(64) }] }), /pin RUSTUP_TOOLCHAIN/);
  await assert.rejects(produceWasmPackageMetadata({ root: path.resolve("."), outDir: path.resolve("."), command: { command: "wasm-pack", args: ["build", "--release", "--no-default-features"], env: { RUSTUP_TOOLCHAIN: "nightly-2026-09-05" } }, sourceSnapshot: [{ path: "Cargo.lock", sha256: "a".repeat(64) }] }), /release builds with default features/);
});

test("metadata filenames stay the two published BASE-root assets", { timeout: 10000 }, () => {
  assert.deepEqual(WASM_PACKAGE_METADATA_FILENAMES, ["wasm-build-provenance.json", "verified-restore-capability.json"]);
  assert.notDeepEqual(wasm, glue);
});


for (const featureArgs of [["--features", "simulation-diagnostics"], ["--features=simulation-diagnostics"], ["-F", "simulation-diagnostics"], ["-Fsimulation-diagnostics"], ["-F=simulation-diagnostics"], ["--no-default-features"], ["--all-features"]]) {
  test(`直接 producer 拒绝显式 feature 参数 ${featureArgs.join(" ")}`, { timeout: 10000 }, async () => {
    await assert.rejects(produceWasmPackageMetadata({ root: path.resolve("."), outDir: path.resolve("."), command: { command: "wasm-pack", args: ["build", "--release", ...featureArgs], env: { RUSTUP_TOOLCHAIN: "nightly-2026-09-05" } }, sourceSnapshot: [{ path: "Cargo.lock", sha256: "a".repeat(64) }] }), /release builds with default features/);
  });
}
