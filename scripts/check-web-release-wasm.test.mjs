import assert from "node:assert/strict";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";
import { verifySharedMemoryArtifact, verifyWebReleaseWasm } from "./check-web-release-wasm.mjs";

// ---------------------------------------------------------------------------
// 手工拼装 wasm 字节 fixture（不依赖外部工具）：
// - section(id, payload) = [id, ...leb32(payload.length), ...payload]
// - name(s) = [...leb32(bytes.length), ...bytes]
// - vec(items) = [...leb32(items.length), ...items.flat()]
// R4a 阶段 1（设计 §6.3 R3-1 末行 / §7.3.2）：产品门 = 时间 import 白名单 +
// 原子码位 tripwire（frontend-build 常驻，必须绿）；共享内存门 =
// verifySharedMemoryArtifact（R3-2 组合入口专用；工具链解阻塞批起
// shared+明确 maximum 为必须绿契约）。
// ---------------------------------------------------------------------------

function leb32(value) {
  if (!Number.isSafeInteger(value) || value < 0) throw new Error(`leb32 需要非负安全整数：${value}`);
  const bytes = [];
  let rest = value;
  do {
    let byte = rest & 0x7f;
    rest = Math.floor(rest / 128);
    if (rest > 0) byte |= 0x80;
    bytes.push(byte);
  } while (rest > 0);
  return bytes;
}

function section(id, payload) {
  return [id, ...leb32(payload.length), ...payload];
}

function name(text) {
  const bytes = [...Buffer.from(text, "utf8")];
  return [...leb32(bytes.length), ...bytes];
}

function vec(entries) {
  return [...leb32(entries.length), ...entries.flat()];
}

/// 空函数体（无原子指令）：locals=0，仅 end。
const plainBody = [0x00, 0x0b];

/// 带 memory.atomic.notify（0xFE 0x00）的函数体：i32.const 0, i32.const 0,
/// notify align=2 offset=0, drop, end。notify 弹出 (addr, count) 返回 i32
/// ——需要 memory，仅用于带 memory import 的 fixture。
const atomicBody = [0x00, 0x41, 0x00, 0x41, 0x00, 0xfe, 0x00, 0x02, 0x00, 0x1a, 0x0b];

/// 带 atomic.fence（0xFE 0x03 + 保留立即数 0）的函数体：不触 memory，
/// 供无 memory import 的产品形态 fixture 使用（产品门不裁 memory 归属）。
const atomicFenceBody = [0x00, 0xfe, 0x03, 0x00, 0x0b];

function funcTypeNoResult() {
  return [0x60, 0x00, 0x00];
}

function funcTypeF64() {
  // () -> f64：时钟 import 的签名（engine_clock_now 返回 f64 ms）。
  return [0x60, 0x00, 0x01, 0x7c];
}

function importFunc(moduleText, fieldText, typeIndex) {
  return [...name(moduleText), ...name(fieldText), 0x00, ...leb32(typeIndex)];
}

/// limits: flags=0x03（shared + max，线程提案要求共享内存必带 max）。
function importSharedMemory(moduleText = "env", fieldText = "memory", minimum = 1, maximum = 32768) {
  return [...name(moduleText), ...name(fieldText), 0x02, 0x03, ...leb32(minimum), ...leb32(maximum)];
}

/// shared 但不带 max（flags=0x02）：形态不完整——共享内存必须带明确
/// maximum（设计 §6.3 R3-1 产物校验行 + threads-xform TX:130 `maximum.unwrap()`
/// 的输入契约），此 fixture 用于断言门拒绝。
function importSharedMemoryNoMax(moduleText = "env", fieldText = "memory", minimum = 1) {
  return [...name(moduleText), ...name(fieldText), 0x02, 0x02, ...leb32(minimum)];
}

/// 非共享内存 import（flags=0x00，仅 min）。
function importPlainMemory(moduleText = "env", fieldText = "memory", minimum = 1) {
  return [...name(moduleText), ...name(fieldText), 0x02, 0x00, ...leb32(minimum)];
}

function wasmModuleBytes({ imports = [], body = plainBody, exports = [] }) {
  const header = [0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
  const typeSection = section(0x01, vec([funcTypeNoResult(), funcTypeF64()]));
  const parts = [header, typeSection];
  if (imports.length > 0) parts.push(section(0x02, vec(imports)));
  parts.push(section(0x03, vec([[0]]))); // 本地函数 1 个，类型 0（() -> ()）
  if (exports.length > 0) parts.push(section(0x07, vec(exports)));
  parts.push(section(0x0a, vec([[...leb32(body.length), ...body]])));
  return Buffer.from(parts.flat());
}

const diagnosticsExportModule = Buffer.from([
  0, 97, 115, 109, 1, 0, 0, 0,
  1, 4, 1, 96, 0, 0,
  3, 2, 1, 0,
  7, 28, 1, 24, ...Buffer.from("npc_decision_diagnostics"), 0, 0,
  10, 4, 1, 2, 0, 11,
]);

const traceExportModule = Buffer.from([
  0, 97, 115, 109, 1, 0, 0, 0,
  1, 4, 1, 96, 0, 0,
  3, 2, 1, 0,
  7, 22, 1, 18, ...Buffer.from("npc_decision_trace"), 0, 0,
  10, 4, 1, 2, 0, 11,
]);

// 产品形态：恰 1 个 env.engine_clock_now + 原子码位（memory 归属不在产品门——
// 共享内存链由组合门承载，阶段 1 显式红登记）。
function productModuleBytes({ clockImports = 1, extraImports = [], body = atomicFenceBody } = {}) {
  const imports = [];
  for (let index = 0; index < clockImports; index += 1) imports.push(importFunc("env", "engine_clock_now", 1));
  imports.push(...extraImports);
  return wasmModuleBytes({ imports, body });
}

// 共享内存形态：产品形态 + shared memory import。
const sharedGlue = "const memory = new WebAssembly.Memory({initial:17, maximum:32768, shared:true});\nexport function boot() { return memory; }\n";

function sharedModuleBytes({ clockImports = 1 } = {}) {
  const imports = [];
  for (let index = 0; index < clockImports; index += 1) imports.push(importFunc("env", "engine_clock_now", 1));
  imports.push(importSharedMemory());
  return wasmModuleBytes({ imports, body: atomicBody });
}

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

test("release WASM checker accepts the threaded product contract", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-threaded.wasm"), productModuleBytes());
    assert.deepEqual(await verifyWebReleaseWasm(assets), ["web_wasm_bg-threaded.wasm"]);
  });
});

test("release WASM checker rejects a second game WASM artifact", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-one.wasm"), productModuleBytes());
    await writeFile(path.join(assets, "web_wasm_bg-two.wasm"), productModuleBytes());
    await assert.rejects(verifyWebReleaseWasm(assets), /恰有一个 game WASM/);
  });
});

test("release WASM checker rejects debug trace exports without instantiating modules", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-debug.wasm"), traceExportModule);
    await assert.rejects(verifyWebReleaseWasm(assets), /npc_decision_trace/);
  });
});

test("release WASM checker rejects any non-whitelisted time import", async () => {
  await withAssets(async (assets) => {
    // env.now：std 弱符号时代的历史胶水名——白名单外零容忍（§7.3.2）。
    await writeFile(path.join(assets, "web_wasm_bg-clock.wasm"), productModuleBytes({
      extraImports: [importFunc("env", "now", 1)],
    }));
    await assert.rejects(verifyWebReleaseWasm(assets), /env\.now/);
  });
});

test("release WASM checker rejects a duplicate engine_clock_now import", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-clock.wasm"), productModuleBytes({ clockImports: 2 }));
    await assert.rejects(verifyWebReleaseWasm(assets), /engine_clock_now/);
  });
});

test("release WASM checker rejects a missing engine_clock_now import", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-clock.wasm"), productModuleBytes({ clockImports: 0 }));
    await assert.rejects(verifyWebReleaseWasm(assets), /engine_clock_now/);
  });
});

test("release WASM checker rejects a module without atomic instructions", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-noatomics.wasm"), productModuleBytes({ body: plainBody }));
    await assert.rejects(verifyWebReleaseWasm(assets), /原子|atomic/i);
  });
});

test("release WASM checker rejects a private module nested anywhere in dist", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-regular.wasm"), productModuleBytes());
    const hidden = path.join(assets, "wasm-diagnostics-pkg");
    await mkdir(hidden);
    await writeFile(path.join(hidden, "private.wasm"), traceExportModule);
    await assert.rejects(verifyWebReleaseWasm(assets), /npc_decision_trace/);
  });
});

test("shared memory gate accepts the shared artifact contract", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-shared.wasm"), sharedModuleBytes());
    await writeFile(path.join(assets, "web_wasm.js"), sharedGlue);
    assert.deepEqual(await verifySharedMemoryArtifact(assets), ["web_wasm_bg-shared.wasm"]);
  });
});

test("shared memory gate accepts minified glue from the bundled dist", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-shared.wasm"), sharedModuleBytes());
    await writeFile(path.join(assets, "chunk-abc123.js"), "const m=new WebAssembly.Memory({initial:17,maximum:32768,shared:!0});");
    assert.deepEqual(await verifySharedMemoryArtifact(assets), ["web_wasm_bg-shared.wasm"]);
  });
});

test("shared memory gate rejects a non-shared memory import", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-serial.wasm"), wasmModuleBytes({
      imports: [importFunc("env", "engine_clock_now", 1), importPlainMemory()],
      body: atomicBody,
    }));
    await writeFile(path.join(assets, "web_wasm.js"), sharedGlue);
    await assert.rejects(verifySharedMemoryArtifact(assets), /shared/);
  });
});

test("shared memory gate rejects a shared memory import without explicit maximum", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-nomax.wasm"), wasmModuleBytes({
      imports: [importFunc("env", "engine_clock_now", 1), importSharedMemoryNoMax()],
      body: atomicBody,
    }));
    await writeFile(path.join(assets, "web_wasm.js"), sharedGlue);
    await assert.rejects(verifySharedMemoryArtifact(assets), /maximum/);
  });
});

test("shared memory gate rejects glue missing the shared memory bootstrap", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-shared.wasm"), sharedModuleBytes());
    await writeFile(path.join(assets, "web_wasm.js"), "export function boot() { return 1; }\n");
    await assert.rejects(verifySharedMemoryArtifact(assets), /WebAssembly\.Memory/);
  });
});


test("shared memory gate rejects historical 4GiB binary maximum", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-shared.wasm"), wasmModuleBytes({
      imports: [importFunc("env", "engine_clock_now", 1), importSharedMemory("env", "memory", 17, 65536)], body: atomicBody,
    }));
    await writeFile(path.join(assets, "web_wasm.js"), sharedGlue);
    await assert.rejects(verifySharedMemoryArtifact(assets), /32768|2GiB/);
  });
});

test("shared memory gate rejects historical 4GiB glue maximum", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-shared.wasm"), sharedModuleBytes());
    await writeFile(path.join(assets, "web_wasm.js"), sharedGlue.replace("maximum:32768", "maximum:65536"));
    await assert.rejects(verifySharedMemoryArtifact(assets), /32768|2GiB/);
  });
});


test("release WASM checker rejects verification facade exports", async () => {
  await withAssets(async (assets) => {
    const bytes = wasmModuleBytes({
      imports: [importFunc("env", "engine_clock_now", 1)], body: atomicFenceBody,
      exports: [[...name("verification_facade_spawn"), 0x00, 0x01]],
    });
    await writeFile(path.join(assets, "web_wasm_bg-verify.wasm"), bytes);
    await assert.rejects(verifyWebReleaseWasm(assets), /verification_facade_spawn/);
  });
});


test("release WASM checker identifies the actual forbidden diagnostics export", async () => {
  await withAssets(async (assets) => {
    await writeFile(path.join(assets, "web_wasm_bg-debug.wasm"), diagnosticsExportModule);
    await assert.rejects(verifyWebReleaseWasm(assets), /npc_decision_diagnostics/);
  });
});


test("release WASM checker rejects 0xFE inside a constant LEB immediate", async () => {
  await withAssets(async (assets) => {
    const bytes = productModuleBytes({ body: [0x00, 0x41, 0xfe, 0x00, 0x1a, 0x0b] });
    assert.equal(WebAssembly.validate(bytes), true);
    await writeFile(path.join(assets, "web_wasm_bg-fakeatomic.wasm"), bytes);
    await assert.rejects(verifyWebReleaseWasm(assets), /原子|atomic/i);
  });
});


test("原子门不把函数名atomic.fence当指令，且不混入同process其他module", async () => {
  const atomic = productModuleBytes();
  const plain = wasmModuleBytes({ imports: [importFunc("env", "engine_clock_now", 1)], body: plainBody,
    exports: [[...name("atomic.fence"), 0x00, 0x01]],
  });
  new WebAssembly.Module(atomic);
  await withAssets(async (good) => withAssets(async (bad) => {
    await writeFile(path.join(good, "web_wasm_bg-atomic.wasm"), atomic);
    await writeFile(path.join(bad, "web_wasm_bg-named.wasm"), plain);
    await assert.rejects(verifyWebReleaseWasm(bad), /原子|atomic/i);
    await verifyWebReleaseWasm(good);
    await Promise.all([verifyWebReleaseWasm(good), assert.rejects(verifyWebReleaseWasm(bad), /原子|atomic/i)]);
    await assert.rejects(verifyWebReleaseWasm(bad), /原子|atomic/i);
  }));
});
