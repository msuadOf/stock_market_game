import { readdir, readFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

async function wasmFiles(directory, prefix = "") {
  const entries = await readdir(path.join(directory, prefix), { withFileTypes: true });
  const nested = await Promise.all(entries.map(async (entry) => {
    const relative = path.join(prefix, entry.name);
    if (entry.isDirectory()) return wasmFiles(directory, relative);
    return entry.isFile() && entry.name.endsWith(".wasm") ? [relative] : [];
  }));
  return nested.flat();
}

async function jsFiles(directory, prefix = "") {
  const entries = await readdir(path.join(directory, prefix), { withFileTypes: true });
  const nested = await Promise.all(entries.map(async (entry) => {
    const relative = path.join(prefix, entry.name);
    if (entry.isDirectory()) return jsFiles(directory, relative);
    return entry.isFile() && entry.name.endsWith(".js") ? [relative] : [];
  }));
  return nested.flat();
}

// R4a 阶段 1（设计输入 §6.3 R3-1 末行 + §7.3.2）：产物门分两层。
//
// **产品门 `verifyWebReleaseWasm`**（frontend-build 常驻，必须绿）：
// 1. 恰有一个 game WASM（web_wasm_bg*.wasm）；
// 2. 无私有诊断导出（npc_decision_*）；
// 3. 时间 import 白名单：整份 import 表允许且仅允许门面自有的
//    `env.engine_clock_now`（f64 ms）恰出现 1 次；其余任何时间类符号
//    （now/timestamp/date/clock/time/performance，含 wasm-bindgen 生成面）
//    零容忍——白名单计数≠1 或名字漂移即构建失败（§7.3.2 断言契约，
//    替代 wasm P0 清单 7 的串行 glue 断言）；
// 4. 原子码位 tripwire：game WASM 的 code section 内必须存在 0xFE 前缀
//    原子指令（钉住 +atomics 锁定链未被漂移）。
//
// **共享内存门 `verifySharedMemoryArtifact`**（R3-2 组合验证入口调用；
// 工具链解阻塞批起恢复为**必须绿**——threads-xform 0.2.93 的 ABI 输入契约
// 已核清并经 link-arg 链满足，见 .tmp/toolchain-unblock/symbol-responsibility.md
// 与 agents/wasm-thread-r4a/toolchain-unblock.md 台账）：
// 5. game WASM 的 memory 必须是 **import 且 shared 且带明确 maximum**
//    （字节级解析——串行形态“模块自导出非共享 memory”与无上限共享
//    memory 在此门下显式拒绝）；
// 6. 目录内至少一个 JS（pkg 的 web_wasm.js 或 dist 中 vite 打包 chunk）
//    包含 `new WebAssembly.Memory(` 且 `shared:` 置真（压缩产物 true→!0
//    两种拼写都认）。

const CLOCK_IMPORT_MODULE = "env";
const CLOCK_IMPORT_NAME = "engine_clock_now";
const timeLikePattern = /now|timestamp|date|clock|time|performance/i;
const sharedMemoryGluePattern = /new\s+WebAssembly\.Memory\s*\(/;
const sharedFlagGluePattern = /shared\s*:\s*(?:true|!0)\b/;

function readLeb32Unsigned(bytes, offset) {
  let value = 0;
  let shift = 0;
  let cursor = offset;
  for (let round = 0; round < 5; round += 1) {
    const byte = bytes[cursor];
    if (byte === undefined) throw new Error(`wasm LEB32 在 ${cursor} 处越界`);
    cursor += 1;
    value += (byte & 0x7f) * 2 ** shift;
    if ((byte & 0x80) === 0) return { value, cursor };
    shift += 7;
  }
  throw new Error("wasm LEB32 超过 5 字节");
}

function readWasmName(bytes, offset) {
  const { value: length, cursor } = readLeb32Unsigned(bytes, offset);
  const end = cursor + length;
  if (end > bytes.length) throw new Error(`wasm name 在 ${cursor} 处越界`);
  return { text: Buffer.from(bytes.subarray(cursor, end)).toString("utf8"), cursor: end };
}

/// 解析 game WASM 的 import 表（字节级，不经 WebAssembly.Module——import
/// 的 shared 标志在 JS 反射面不可见）。
function parseWasmImports(bytes) {
  if (bytes.length < 8 || bytes[0] !== 0x00 || bytes[1] !== 0x61) throw new Error("非 wasm 二进制（magic 不符）");
  let cursor = 8;
  const imports = [];
  while (cursor < bytes.length) {
    const sectionId = bytes[cursor];
    const { value: sectionSize, cursor: afterSize } = readLeb32Unsigned(bytes, cursor + 1);
    const sectionEnd = afterSize + sectionSize;
    if (sectionEnd > bytes.length) throw new Error(`wasm section ${sectionId} 声明长度越界`);
    if (sectionId === 0x02) {
      const { value: importCount, cursor: listStart } = readLeb32Unsigned(bytes, afterSize);
      let entry = listStart;
      for (let index = 0; index < importCount; index += 1) {
        const module = readWasmName(bytes, entry);
        const field = readWasmName(bytes, module.cursor);
        const kind = bytes[field.cursor];
        if (kind === undefined) throw new Error("wasm import 条目截断");
        entry = field.cursor + 1;
        let memory = null;
        if (kind === 0x02) {
          const flags = bytes[entry];
          if (flags === undefined) throw new Error("wasm memory limits 截断");
          entry += 1;
          const minimum = readLeb32Unsigned(bytes, entry);
          entry = minimum.cursor;
          let maximum;
          if (flags & 0x01) {
            maximum = readLeb32Unsigned(bytes, entry);
            entry = maximum.cursor;
          }
          // 线程提案：shared 位 = 0x02（且要求 max，即 flags===0x03 合法形）。
          memory = { shared: (flags & 0x02) !== 0, minimum: minimum.value, maximum: maximum?.value };
        } else if (kind === 0x00) {
          const typeIndex = readLeb32Unsigned(bytes, entry);
          entry = typeIndex.cursor;
        } else if (kind === 0x01 || kind === 0x03) {
          let walk = entry;
          for (let round = 0; round < 2; round += 1) {
            const limit = readLeb32Unsigned(bytes, walk);
            walk = limit.cursor;
            if ((bytes[entry] & 0x01) === 0) break;
          }
          entry = walk;
        }
        imports.push({ module: module.text, field: field.text, kind, memory });
      }
    }
    cursor = sectionEnd;
  }
  return imports;
}

/// code section（id=10）内扫描 0xFE 前缀原子指令（子码 ≤0x4F：notify/
/// wait/加载/存储/RMW 全族；其余 0xFE 高码位属 GC/relaxed-SIMD 等后继提案）。
function containsAtomicInstructions(bytes) {
  let cursor = 8;
  while (cursor < bytes.length) {
    const sectionId = bytes[cursor];
    const { value: sectionSize, cursor: afterSize } = readLeb32Unsigned(bytes, cursor + 1);
    const sectionEnd = afterSize + sectionSize;
    if (sectionEnd > bytes.length) throw new Error(`wasm section ${sectionId} 声明长度越界`);
    if (sectionId === 0x0a) {
      for (let index = afterSize; index < sectionEnd - 1; index += 1) {
        if (bytes[index] === 0xfe && bytes[index + 1] <= 0x4f) return true;
      }
    }
    cursor = sectionEnd;
  }
  return false;
}

async function verifyTimeImportWhitelist(bytes) {
  // 3. 时间 import 白名单（唯一允许：env.engine_clock_now 恰 1 次）。
  const imports = parseWasmImports(bytes);
  const timeLikeImports = imports.filter(({ module, field }) => timeLikePattern.test(`${module}.${field}`));
  const clockImports = timeLikeImports.filter(({ module, field }) => module === CLOCK_IMPORT_MODULE && field === CLOCK_IMPORT_NAME);
  const alienTimeImports = timeLikeImports.filter((entry) => entry !== clockImports[0] || clockImports.length !== 1);
  if (clockImports.length !== 1) {
    throw new Error(`game WASM 时间 import 白名单违规：env.${CLOCK_IMPORT_NAME} 期望恰 1 次实际 ${clockImports.length} 次（时钟门面未接线或重复引入）`);
  }
  if (alienTimeImports.length > 0) {
    const names = alienTimeImports.map(({ module, field }) => `${module}.${field}`).join(", ");
    throw new Error(`game WASM 携带白名单外时间类 import：${names}（std 时间胶水/第三方时间面零容忍——设计 §7.3.2）`);
  }
}

export async function verifyWebReleaseWasm(directory) {
  const modules = await wasmFiles(directory);
  const files = modules
    .filter((name) => /^web_wasm_bg(?:-[^/]*)?\.wasm$/.test(path.basename(name)))
    .sort();
  if (files.length !== 1) {
    throw new Error(`release assets 必须恰有一个 game WASM，实际为 ${files.length}: ${files.join(", ")}`);
  }
  await Promise.all(modules.map(async (filename) => {
    const bytes = await readFile(path.join(directory, filename));
    const module = new WebAssembly.Module(bytes);
    const forbidden = WebAssembly.Module.exports(module).filter(({ name }) => /npc_decision_(trace|diagnostics)/.test(name));
    if (forbidden.length > 0) throw new Error(`release WASM 暴露私有诊断导出 ${forbidden.map(({ name }) => name).join(", ")}: ${filename}`);
  }));
  const gameBytes = new Uint8Array(await readFile(path.join(directory, files[0])));
  await verifyTimeImportWhitelist(gameBytes);
  // 4. 原子码位（tripwire）。
  if (!containsAtomicInstructions(gameBytes)) {
    throw new Error("game WASM code section 无 0xFE 原子指令：+atomics 产物链疑似漂移为串行链");
  }
  return files;
}

/// 共享内存门（R3-2 组合验证入口专用；阶段 1 显式红承载——依据与登记见
/// apps/web-wasm/.cargo/config.toml 注释与 agents/wasm-thread-r4a/ 台账）。
export async function verifySharedMemoryArtifact(directory) {
  const modules = await wasmFiles(directory);
  const files = modules
    .filter((name) => /^web_wasm_bg(?:-[^/]*)?\.wasm$/.test(path.basename(name)))
    .sort();
  if (files.length !== 1) {
    throw new Error(`release assets 必须恰有一个 game WASM，实际为 ${files.length}: ${files.join(", ")}`);
  }
  const bytes = new Uint8Array(await readFile(path.join(directory, files[0])));
  const imports = parseWasmImports(bytes);
  const memoryImports = imports.filter((entry) => entry.kind === 0x02);
  if (memoryImports.length === 0) {
    throw new Error("game WASM 未 import memory：共享内存形态（import + shared）未产出——需 --import-memory --shared-memory link-arg 链（设计 §6.3 R3-1）");
  }
  const nonShared = memoryImports.filter((entry) => entry.memory === null || !entry.memory.shared);
  if (nonShared.length > 0) {
    const names = nonShared.map(({ module, field }) => `${module}.${field}`).join(", ");
    throw new Error(`game WASM 的 memory import 非 shared：${names}（缺 --shared-memory link-arg 或链漂移）`);
  }
  // 共享内存必须带明确 maximum（设计 §6.3 R3-1 产物校验行；亦是
  // threads-xform 输入契约 TX:130 `mem.maximum.unwrap()` 的前置）。
  const noMax = memoryImports.filter((entry) => entry.memory.maximum === undefined);
  if (noMax.length > 0) {
    const names = noMax.map(({ module, field }) => `${module}.${field}`).join(", ");
    throw new Error(`game WASM 的 shared memory import 缺明确 maximum：${names}（缺 --max-memory link-arg——共享内存无上限在 JS 侧无法构造）`);
  }
  const glueFiles = await jsFiles(directory);
  let glueFound = false;
  for (const filename of glueFiles) {
    const glue = await readFile(path.join(directory, filename), "utf8");
    if (sharedMemoryGluePattern.test(glue) && sharedFlagGluePattern.test(glue)) {
      glueFound = true;
      break;
    }
  }
  if (!glueFound) {
    throw new Error("产物目录 JS 缺少 shared WebAssembly.Memory 引导（new WebAssembly.Memory + shared:true——设计 §6.3 R3-1 产物校验行）");
  }
  return files;
}

if (process.argv[1] !== undefined && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const directory = process.argv[2] ?? fileURLToPath(new URL("../apps/web/dist", import.meta.url));
  const files = await verifyWebReleaseWasm(directory);
  process.stdout.write(`release WASM verified: ${files.join(", ")}\n`);
}
