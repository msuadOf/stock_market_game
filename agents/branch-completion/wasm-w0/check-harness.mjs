// dev harness只核实际ABI、共享内存预算与feature导出隔离，不充作release或browser验收。
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { verifyWebReleaseWasm, verifySharedMemoryArtifact } from "../../../scripts/check-web-release-wasm.mjs";
const directory = "/data1/baiyifan/workplace/stock_market_game/.tmp/wasm-w0-recovery/artifacts/harness-dev/pkg";
console.log(JSON.stringify({node:process.version,v8:process.versions.v8,profile:"dev",directory}));
const bytes = await readFile(`${directory}/web_wasm_bg.wasm`);
const module = new WebAssembly.Module(bytes);
const exports = WebAssembly.Module.exports(module).map(({name})=>name);
for (const name of ["verification_facade_spawn","verification_facade_join","facade_slot_init","facade_slot_layout","facade_child_trampoline"]) assert.ok(exports.includes(name),`harness缺导出${name}`);
assert.ok(WebAssembly.Module.imports(module).some(({module,name})=>module === "env" && name === "engine_clock_now"));
await verifySharedMemoryArtifact(directory);
await assert.rejects(verifyWebReleaseWasm(directory), /verification_facade_/);
for (const name of ["web_wasm_bg.wasm", "web_wasm.js"]) {
  const data = await readFile(`${directory}/${name}`);
  console.log(JSON.stringify({name,bytes:data.length,sha256:createHash("sha256").update(data).digest("hex")}));
}
console.log(JSON.stringify({verificationExports:exports.filter((name)=>name.startsWith("verification_facade_")),gates:"dev ABI/shared import/2GiB binary+glue/正式门拒绝验证包均通过"}));
