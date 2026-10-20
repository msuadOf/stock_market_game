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

// 串行形态 glue 契约（wasm P0 清单 7，承接退役的 check-wasm-threading.mjs）：
// 串行 wasm 的 memory 由模块自导出，glue（web_wasm.js / web_wasm_bg.js）不得
// 在 JS 侧创建 WebAssembly.Memory，也不得声明 shared memory / 线程栈。
const serialGlue = "import * as wasm from './web_wasm_bg.wasm';\nexport function civil_date(handle) { return wasm.civil_date(handle); }\n";

test("release WASM checker accepts the serial glue contract", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-regular.wasm"), emptyModule);
    await writeFile(path.join(assets, "web_wasm.js"), serialGlue);
    assert.deepEqual(await verifyWebReleaseWasm(assets), ["web_wasm_bg-regular.wasm"]);
  });
});

test("release WASM checker rejects glue claiming shared memory", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-regular.wasm"), emptyModule);
    await writeFile(path.join(assets, "web_wasm.js"), `${serialGlue}new WebAssembly.Memory({initial:22,maximum:65536,shared:true});\n`);
    await assert.rejects(verifyWebReleaseWasm(assets), /shared/);
  });
});

test("release WASM checker rejects glue carrying the threaded stack contract", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-regular.wasm"), emptyModule);
    await writeFile(path.join(assets, "web_wasm_bg.js"), `${serialGlue}export function thread_stack_size() {}\n`);
    await assert.rejects(verifyWebReleaseWasm(assets), /thread_stack_size/);
  });
});

test("release WASM checker rejects glue creating memory on the JS side", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-regular.wasm"), emptyModule);
    await writeFile(path.join(assets, "web_wasm.js"), `${serialGlue}const memory = new WebAssembly.Memory({initial:16});\n`);
    await assert.rejects(verifyWebReleaseWasm(assets), /JS 侧创建 WebAssembly\.Memory/);
  });
});
