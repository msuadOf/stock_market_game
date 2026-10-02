import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";

test("App preserves structured host failure details until the feedback view", () => {
  const source = readFileSync(new URL("../App.tsx", import.meta.url), "utf8");
  assert.ok(source.includes("useState<string | HostFailure | null>(null)"));
  assert.equal(source.includes("fatalHostErrorRef.current(`${failure.code}"), false);
  assert.ok(source.includes("setError(failure)"));
});
