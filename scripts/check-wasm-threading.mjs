import fs from "node:fs";
import path from "node:path";

const gluePath = path.resolve(process.argv[2] ?? "apps/web-wasm/pkg/web_wasm.js");
const glue = fs.readFileSync(gluePath, "utf8");
const requiredFragments = [
  "new WebAssembly.Memory({",
  "shared:true",
  "imports.wbg.memory = memory",
  "thread_stack_size",
];
const missing = requiredFragments.filter((fragment) => !glue.includes(fragment));
if (missing.length > 0) {
  throw new Error(
    `WASM threading contract is missing from ${gluePath}: ${missing.join(", ")}. ` +
      "The generated memory would not be safely shared with wasm-bindgen-rayon workers.",
  );
}

console.log(`WASM threading contract verified: ${gluePath}`);
