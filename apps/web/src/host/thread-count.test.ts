import assert from "node:assert/strict";
import test from "node:test";
import { resolveThreadCount } from "./thread-count.ts";

test("WASM pool uses the browser-reported capacity", () => {
  assert.equal(resolveThreadCount(16), 16);
});

test("WASM pool rejects missing or invalid capacity instead of silently choosing a serial fallback", () => {
  for (const invalid of [undefined, 0, -1, 1.5, Infinity, 2 ** 32, Number.MAX_SAFE_INTEGER + 1]) {
    assert.throws(() => resolveThreadCount(invalid), /hardwareConcurrency.*正安全整数/);
  }
});
