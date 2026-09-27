import assert from "node:assert/strict";
import { EventEmitter } from "node:events";
import { it } from "node:test";

import { ORDINARY_TEST_MAX_MS, parseArgs, runBoundedCommand, runWithDeadline } from "./run-with-deadline.mjs";

it("accepts an ordinary command that finishes inside the ten-second gate", async () => {
  await runWithDeadline({
    command: process.execPath,
    args: ["-e", "process.exit(0)"],
    timeoutMs: 1_000,
  });
});

it("rejects attempts to weaken the ten-second gate", () => {
  assert.throws(
    () => runWithDeadline({ command: process.execPath, timeoutMs: ORDINARY_TEST_MAX_MS + 1 }),
    /10000ms/,
  );
  assert.deepEqual(parseArgs(["10000", "--", "node", "--version"]), {
    timeoutMs: 10_000,
    command: "node",
    args: ["--version"],
  });
});

it("terminates an over-time ordinary command", async () => {
  await assert.rejects(
    runWithDeadline({
      command: process.execPath,
      args: ["-e", "setInterval(() => {}, 1000)"],
      timeoutMs: 50,
    }),
    /total 50ms deadline.*process tree/i,
  );
});

it("uses a referenced second cutoff when a killed child never emits close", async () => {
  const child = new EventEmitter();
  child.pid = undefined;
  const startedAt = Date.now();
  await assert.rejects(
    runBoundedCommand({
      command: "never-closes",
      timeoutMs: 50,
      cleanupReserveMs: 10,
      spawnProcess: () => child,
    }),
    /total 50ms deadline.*close did not settle.*10ms cleanup reserve/i,
  );
  const elapsedMs = Date.now() - startedAt;
  assert.ok(elapsedMs >= 40 && elapsedMs < 250, `unexpected hard-cutoff wall time: ${elapsedMs}ms`);
});

it("captures stdout and stderr for build tools without shell redirection", async () => {
  const result = await runBoundedCommand({
    command: process.execPath,
    args: ["-e", "process.stdout.write('artifact\\n'); process.stderr.write('diagnostic\\n')"],
    timeoutMs: 1_000,
    captureOutput: true,
  });
  assert.equal(result.stdout, "artifact\n");
  assert.equal(result.stderr, "diagnostic\n");
});

it("delivers captured child output before a real child times out", async () => {
  const stdoutChunks = [];
  const stderrChunks = [];
  await assert.rejects(runBoundedCommand({
    command: process.execPath,
    args: ["-e", "process.stdout.write('artifact\\n'); process.stderr.write('Compiling fixture\\n'); setInterval(() => {}, 1000)"],
    timeoutMs: 500,
    cleanupReserveMs: 100,
    captureOutput: true,
    onStdout: (chunk) => stdoutChunks.push(chunk.toString()),
    onStderr: (chunk) => stderrChunks.push(chunk.toString()),
  }), /total 500ms deadline.*process tree/i);
  assert.equal(stdoutChunks.join(""), "artifact\n");
  assert.equal(stderrChunks.join(""), "Compiling fixture\n");
});

it("delivers captured failure diagnostics before rejecting a real child", async () => {
  const stderrChunks = [];
  await assert.rejects(runBoundedCommand({
    command: process.execPath,
    args: ["-e", "process.stderr.write('error: compiler failed\\n'); process.exit(2)"],
    timeoutMs: 1_000,
    captureOutput: true,
    onStderr: (chunk) => stderrChunks.push(chunk.toString()),
  }), /exited with 2.*compiler failed/i);
  assert.equal(stderrChunks.join(""), "error: compiler failed\n");
});

it("terminates a real child and reports a streaming callback failure", async () => {
  await assert.rejects(runBoundedCommand({
    command: process.execPath,
    args: ["-e", "process.stdout.write('data'); setInterval(() => {}, 1000)"],
    timeoutMs: 1_000,
    captureOutput: true,
    onStdout: () => { throw null; },
  }), /stdout callback failed: null/);
});
