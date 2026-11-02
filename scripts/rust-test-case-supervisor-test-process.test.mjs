import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { it } from "node:test";
import { killRecordedProcesses, readProcessBirth, recordProcessIdentity } from "./rust-test-case-supervisor-test-process.mjs";

it("清理记录必须先核对 PID birth，身份不符不得杀", { timeout: 10000, skip: process.platform !== "linux" }, async () => {
  const child = spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], { stdio: "ignore" });
  let identity;
  try {
    await once(child, "spawn");
    identity = recordProcessIdentity(child.pid);
    const wrongBirth = String(Number(identity.birth) + 1);
    const refused = killRecordedProcesses([{ pid: identity.pid, birth: wrongBirth }]);
    assert.equal(refused.length, 1);
    assert.equal(refused[0].status, "mismatch");
    assert.notEqual(readProcessBirth(identity.pid), undefined, "身份不符时不得杀死目标进程");
  } finally {
    if (identity !== undefined) killRecordedProcesses([identity]);
    try { child.kill("SIGKILL"); } catch (error) { if (error.code !== "ESRCH") throw error; }
    await once(child, "close").catch(() => undefined);
  }
});
