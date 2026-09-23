import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import test from "node:test";

const script = resolve("scripts/desktop/validate-wayland-png.py");
test("Wayland PNG validator defines a decoded non-black app-region contract", () => {
  const source = readFileSync(script, "utf8");
  assert.match(source, /capture is entirely black/);
  assert.match(source, /no app-visible bright region below the shell panel/);
  assert.match(source, /source\.verify\(\)/);
  assert.match(source, /get_flattened_data\(\)/);
});

test("Wayland PNG validator rejects an invalid PNG", () => {
  const invalidCapture = resolve(tmpdir(), `wayland-invalid-${process.pid}.png`);
  writeFileSync(invalidCapture, "not a PNG");
  try {
    assert.throws(
      () => execFileSync("uv", ["run", script, invalidCapture, "1280", "800"], { encoding: "utf8", stdio: "pipe" }),
      /cannot identify image file/,
    );
  } finally {
    rmSync(invalidCapture, { force: true });
  }
});
