import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("Given a running direct WASM host, when a save is restored, then it uses the atomic restore transaction", () => {
  const source = readFileSync(new URL("./wasm-host.ts", import.meta.url), "utf8");
  const loadStart = source.indexOf("async load(slot)");
  const runningCapture = source.indexOf("const wasRunning = timer !== null;", loadStart);
  const transaction = source.indexOf("restoreWasmSession({", loadStart);
  assert.ok(loadStart >= 0 && runningCapture > loadStart && transaction > runningCapture);
});

test("Given the current JSON restore boundary, when typed, then both direct and generated WASM APIs declare restore_json", () => {
  const bindings = readFileSync(new URL("../wasm-pkg.d.ts", import.meta.url), "utf8");
  const api = readFileSync(new URL("../types/engine.ts", import.meta.url), "utf8");
  assert.match(bindings, /restore_json\(saveJson: string\): number/);
  assert.match(api, /restore_json\(saveJson: string\): number/);
});

test("Given the civil-day barrier contract, when stepping, then WASM hosts consume the next typed update without classifying error text", () => {
  const direct = readFileSync(new URL("./wasm-host.ts", import.meta.url), "utf8");
  const worker = readFileSync(new URL("./wasm-worker.ts", import.meta.url), "utf8");

  assert.match(direct, /const rawUpdate = wasm\.step\(handle\)/);
  assert.match(worker, /const rawUpdate = wasm\.step\(session\)/);
  assert.match(direct, /inspectWasmUpdateDelivery\(rawUpdate, preferences\)/);
  assert.match(worker, /inspectWasmUpdateDelivery\(rawUpdate, pausePreferences\)/);
  assert.match(direct, /if \(publish\(rawUpdate\)\) speedMeter\.recordTicks\(\)/);
  assert.match(worker, /if \(publish\(rawUpdate\)\) speedMeter\.recordTicks\(\)/);
  assert.doesNotMatch(direct, /civil day barrier must be published before stepping/);
  assert.doesNotMatch(worker, /civil day barrier must be published before stepping/);
});
