import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";

const script = resolve("scripts/desktop/wayland-ipc-probe.mjs");

test("Wayland IPC probe requires a loopback inspector port and explicit expected feature state", () => {
  assert.throws(
    () => execFileSync("node", [script], { encoding: "utf8", stdio: "pipe" }),
    /usage: node scripts\/desktop\/wayland-ipc-probe\.mjs/,
  );
  const source = readFileSync(script, "utf8");
  assert.match(source, /127\.0\.0\.1/);
  assert.match(source, /npc_decision_diagnostics/);
  assert.match(source, /toLowerCase\(\)/);
  assert.match(source, /Target\.sendMessageToTarget/);
  assert.match(source, /Target\.dispatchMessageFromTarget/);
  assert.match(source, /Tauri WebView IPC bridge did not become ready/);
  assert.match(source, /malformedRejected/);
  assert.match(source, /staleRejected/);
  assert.match(source, /hasRecordsProperty/);
  assert.match(source, /records > 128/);
});
