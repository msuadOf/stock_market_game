import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { verifyWebReleaseWasm } from "./check-web-release-wasm.mjs";

const emptyModule = Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]);
const traceExportModule = Buffer.from([
  0, 97, 115, 109, 1, 0, 0, 0,
  1, 4, 1, 96, 0, 0,
  3, 2, 1, 0,
  7, 22, 1, 18, ...Buffer.from("npc_decision_trace"), 0, 0,
  10, 4, 1, 2, 0, 11,
]);
const diagnosticsExportModule = Buffer.from([
  0, 97, 115, 109, 1, 0, 0, 0,
  1, 4, 1, 96, 0, 0,
  3, 2, 1, 0,
  7, 28, 1, 24, ...Buffer.from("npc_decision_diagnostics"), 0, 0,
  10, 4, 1, 2, 0, 11,
]);

async function withAssets(run) {
  const root = await mkdtemp(path.join(os.tmpdir(), "web-release-wasm-"));
  const assets = path.join(root, "assets");
  await mkdir(assets);
  try {
    await run(assets);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}

test("release WASM checker accepts one module without private trace exports", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-regular.wasm"), emptyModule);
    assert.deepEqual(await verifyWebReleaseWasm(assets), ["web_wasm_bg-regular.wasm"]);
  });
});

test("release WASM checker rejects debug trace exports without instantiating modules", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-debug.wasm"), traceExportModule);
    await assert.rejects(verifyWebReleaseWasm(assets), /npc_decision_trace/);
  });
});

test("release WASM checker rejects a second game WASM artifact", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-one.wasm"), emptyModule);
    await writeFile(path.join(assets, "web_wasm_bg-two.wasm"), emptyModule);
    await assert.rejects(verifyWebReleaseWasm(assets), /恰有一个 game WASM/);
  });
});

test("release WASM checker identifies the actual forbidden diagnostics export", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-debug.wasm"), diagnosticsExportModule);
    await assert.rejects(verifyWebReleaseWasm(assets), /npc_decision_diagnostics/);
  });
});

test("release WASM checker rejects a private module nested anywhere in dist", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-regular.wasm"), emptyModule);
    const hidden = path.join(assets, "wasm-diagnostics-pkg");
    await mkdir(hidden);
    await writeFile(path.join(hidden, "private.wasm"), traceExportModule);
    await assert.rejects(verifyWebReleaseWasm(assets), /npc_decision_trace/);
  });
});
