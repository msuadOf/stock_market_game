import assert from "node:assert/strict";
import test from "node:test";
import { shouldLoadDiagnosticsWasm } from "./wasm-build-mode.ts";

test("diagnostics WASM loads only in DEV with its explicit feature flag", () => {
  assert.equal(shouldLoadDiagnosticsWasm(false, "1"), false);
  assert.equal(shouldLoadDiagnosticsWasm(true, undefined), false);
  assert.equal(shouldLoadDiagnosticsWasm(true, "0"), false);
  assert.equal(shouldLoadDiagnosticsWasm(true, "1"), true);
});
