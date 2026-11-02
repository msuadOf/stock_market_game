import { createHash } from "node:crypto";
import { readFileSync, writeFileSync } from "node:fs";
import { parseSaveJson } from "../../apps/web/src/save/save-schema.ts";

const input = "packages/engine/tests/fixtures/correction/current-completed-save.json";
const output = "agents/calendar-fixture-integration-20261103/evidence/web-roundtrip-current.json";
const sha256 = (bytes) => createHash("sha256").update(bytes).digest("hex");
const inputBytes = readFileSync(input);
const text = inputBytes.toString("utf8");
const parsed = parseSaveJson(text);
const outputText = JSON.stringify(parsed);
writeFileSync(output, outputText);
console.log(JSON.stringify({
  input,
  inputSha256: sha256(inputBytes),
  output,
  outputSha256: sha256(outputText),
  inputBytes: inputBytes.length,
  outputBytes: Buffer.byteLength(outputText),
}));
