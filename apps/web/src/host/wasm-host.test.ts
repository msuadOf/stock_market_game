import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("Given a running direct WASM host, when a save is restored, then it captures the pre-stop run state and restarts it", () => {
  const source = readFileSync(new URL("./wasm-host.ts", import.meta.url), "utf8");
  const loadStart = source.indexOf("async load(slot)");
  const runningCapture = source.indexOf("const wasRunning = timer !== null;", loadStart);
  const stop = source.indexOf("if (wasRunning) {", loadStart);
  const restart = source.indexOf("if (wasRunning) startTimer();", loadStart);
  assert.ok(loadStart >= 0 && runningCapture > loadStart && stop > runningCapture && restart > stop);
});

test("Given the current JSON restore boundary, when typed, then both direct and generated WASM APIs declare restore_json", () => {
  const bindings = readFileSync(new URL("../wasm-pkg.d.ts", import.meta.url), "utf8");
  const api = readFileSync(new URL("../types/engine.ts", import.meta.url), "utf8");
  assert.match(bindings, /restore_json\(saveJson: string\): number/);
  assert.match(api, /restore_json\(saveJson: string\): number/);
});
