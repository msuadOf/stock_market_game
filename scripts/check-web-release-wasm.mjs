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

// wasm P0 清单 7（承接退役的 check-wasm-threading.mjs 产物断言，二选一已裁走本脚本）：
// 当前 wasm 为串行形态——wasm-pack 产物目录的 glue（web_wasm.js / web_wasm_bg.js，
// dist 中 glue 已被 vite 打进 chunk、无独立文件，断言作用于产物目录调用点）不得
// 携带任何线程契约片段：串行 wasm 的 memory 由模块自导出，JS 侧不创建
// WebAssembly.Memory，也不声明 shared memory / 线程栈；真线程链落地（R3/R4）时
// 按设计输入 §7.3.2 白名单断言替换本段。
const threadedGlueForbidden = [/new\s+WebAssembly\.Memory\s*\(/, /shared\s*:\s*true/, /thread_stack_size/];

async function verifySerialGlueContract(directory) {
  const glues = (await jsFiles(directory)).filter((name) => /^web_wasm(?:_bg)?\.js$/.test(path.basename(name)));
  await Promise.all(glues.map(async (filename) => {
    const glue = await readFile(path.join(directory, filename), "utf8");
    const leftover = threadedGlueForbidden.filter((pattern) => pattern.test(glue)).map(String);
    if (leftover.length > 0) throw new Error(`release WASM glue ${filename} 携带线程契约片段 ${leftover.join(", ")}：串行 wasm 产物不得在 JS 侧创建 WebAssembly.Memory，也不得声明 shared memory / 线程栈`);
  }));
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
  await verifySerialGlueContract(directory);
  return files;
}

if (process.argv[1] !== undefined && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const directory = process.argv[2] ?? fileURLToPath(new URL("../apps/web/dist", import.meta.url));
  const files = await verifyWebReleaseWasm(directory);
  process.stdout.write(`release WASM verified: ${files.join(", ")}\n`);
}
