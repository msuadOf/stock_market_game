import assert from "node:assert/strict";
import { mkdir, readdir } from "node:fs/promises";
import path from "node:path";
import { test } from "node:test";
import { main } from "./frontend-build.mjs";
import { runBoundedCommand } from "./run-with-deadline.mjs";

test("frontend supervisor 的共享 deadline 覆盖 worker 及被强杀后的 sysroot 清理", { timeout: 10000 }, async () => {
  let temporary;
  await assert.rejects(main(["--jobs", "4"], { timeoutMs: 1500, output: () => {}, run: async (command) => {
    temporary = command.env.STOCK_FRONTEND_SYSROOT_TEMP_ROOT;
    await mkdir(path.join(temporary, "sysroot"));
    assert.ok(command.timeoutMs <= 1500 && command.timeoutMs > 0);
    const source = 'require("node:child_process").spawn(process.execPath, ["-e", "setInterval(() => {}, 1000)"], {stdio:"inherit"}); setInterval(() => {}, 1000)';
    await runBoundedCommand({ ...command, command: process.execPath, args: ["-e", source], timeoutMs: 500, cleanupReserveMs: 200 });
  } }), (error) => { assert.match(error.message, /bounded command/); return true; });
  await assert.rejects(readdir(temporary), { code: "ENOENT" });
});
