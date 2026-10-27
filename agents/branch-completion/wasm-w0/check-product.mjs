// 对正式WASM产物执行真实门并记录身份，不实例化引擎。
import { readFile } from "node:fs/promises";
import { createHash } from "node:crypto";
import { verifyWebReleaseWasm, verifySharedMemoryArtifact } from "../../../scripts/check-web-release-wasm.mjs";
const directory = "/data1/baiyifan/workplace/stock_market_game/.tmp/wasm-w0-recovery/artifacts/product/pkg";
console.log(JSON.stringify({node:process.version,v8:process.versions.v8,directory}));
await verifyWebReleaseWasm(directory);
await verifySharedMemoryArtifact(directory);
for (const name of ["web_wasm_bg.wasm", "web_wasm.js"]) {
  const bytes = await readFile(`${directory}/${name}`);
  console.log(JSON.stringify({name,bytes:bytes.length,sha256:createHash("sha256").update(bytes).digest("hex")}));
}
console.log("正式product时间/真实atomic/私有导出隔离/shared import/2GiB binary+glue门通过");
