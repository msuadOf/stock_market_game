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
  return files;
}

if (process.argv[1] !== undefined && import.meta.url === new URL(`file://${process.argv[1]}`).href) {
  const directory = process.argv[2] ?? fileURLToPath(new URL("../apps/web/dist", import.meta.url));
  const files = await verifyWebReleaseWasm(directory);
  process.stdout.write(`release WASM verified: ${files.join(", ")}\n`);
}
