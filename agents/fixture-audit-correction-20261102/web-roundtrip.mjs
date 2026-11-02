import { readFileSync, writeFileSync } from "node:fs";
import { parseSaveJson } from "../../apps/web/src/save/save-schema.ts";

const [input, output] = process.argv.slice(2);
if (input === undefined || output === undefined) {
  throw new Error("用法：node web-roundtrip.mjs <输入SaveSlot> <输出路径>");
}
const parsed = parseSaveJson(readFileSync(input, "utf8"));
const encoded = JSON.stringify(parsed);
writeFileSync(output, encoded, { flag: "wx" });
console.log(JSON.stringify({ input, output, bytes: Buffer.byteLength(encoded) }));
