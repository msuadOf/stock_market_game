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

it("rejects an already aborted signal before spawning a child", async () => {
  const controller = new AbortController();
  const reason = new Error("sibling failed before start");
  controller.abort(reason);
  let spawns = 0;
  await assert.rejects(runBoundedCommand({
    command: "must-not-start", timeoutMs: 100,
    signal: controller.signal,
    spawnProcess: () => { spawns += 1; },
  }), (error) => error === reason);
  assert.equal(spawns, 0);
});

it("waits for child close after abort and removes the signal listener on settlement", async () => {
  const child = new EventEmitter();
  const controller = new AbortController();
  const reason = new Error("sibling failed during execution");
  let settled = false;
  const result = runBoundedCommand({
    command: "in-flight", timeoutMs: 200,
    signal: controller.signal, spawnProcess: () => child,
  });
  const rejection = assert.rejects(result, (error) => {
    assert.match(error.message, /was aborted because a sibling command failed/);
    assert.equal(error.cause, reason);
    return true;
  }).then(() => { settled = true; });
  controller.abort(reason);
  await Promise.resolve();
  assert.equal(settled, false);
  child.emit("close", null, "SIGKILL");
  await rejection;
});

it("settles once when close, error and a later abort repeat completion events", async () => {
  const child = new EventEmitter();
  const controller = new AbortController();
  let listenerRemovals = 0;
  const originalRemove = controller.signal.removeEventListener.bind(controller.signal);
  controller.signal.removeEventListener = (...args) => {
    listenerRemovals += 1;
    return originalRemove(...args);
  };
  const result = runBoundedCommand({
    command: "repeat-events", timeoutMs: 200,
    signal: controller.signal, spawnProcess: () => child,
  });
  child.emit("close", 0, null);
  child.emit("error", new Error("late error"));
  controller.abort(new Error("late sibling failure"));
  child.emit("close", 2, null);
  assert.deepEqual(await result, { stdout: "", stderr: "" });
  assert.equal(listenerRemovals, 1);
});

it("preserves a spawn error when close and abort arrive after rejection", async () => {
  const child = new EventEmitter();
  const controller = new AbortController();
  const result = runBoundedCommand({
    command: "missing-command", timeoutMs: 200,
    signal: controller.signal, spawnProcess: () => child,
  });
  const rejection = assert.rejects(result, /cannot start ordinary test command missing-command: injected spawn failure/);
  child.emit("error", new Error("injected spawn failure"));
  child.emit("close", 0, null);
  controller.abort(new Error("late sibling failure"));
  await rejection;
});

it("keeps a streaming callback error ahead of a sibling abort on close", async () => {
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  const controller = new AbortController();
  const result = runBoundedCommand({
    command: "stream-failure", timeoutMs: 200,
    signal: controller.signal, spawnProcess: () => child,
    captureOutput: true,
    onStderr: () => { throw new Error("injected callback failure"); },
  });
  const rejection = assert.rejects(result, /stderr callback failed: injected callback failure/);
  child.stderr.emit("data", Buffer.from("diagnostic"));
  controller.abort(new Error("sibling failure"));
  child.emit("close", null, "SIGKILL");
  await rejection;
});

it("accumulates captured streams and passes their original chunks to callbacks", async () => {
  const child = new EventEmitter();
  child.stdout = new EventEmitter();
  child.stderr = new EventEmitter();
  const chunks = [Buffer.from("artifact"), Buffer.from("\n"), Buffer.from("diagnostic")];
  const stdoutChunks = [];
  const stderrChunks = [];
  const result = runBoundedCommand({
    command: "captured-streams", timeoutMs: 200, captureOutput: true,
    spawnProcess: () => child,
    onStdout: (chunk) => stdoutChunks.push(chunk),
    onStderr: (chunk) => stderrChunks.push(chunk),
  });
  child.stdout.emit("data", chunks[0]);
  child.stdout.emit("data", chunks[1]);
  child.stderr.emit("data", chunks[2]);
  child.emit("close", 0, null);
  assert.deepEqual(await result, { stdout: "artifact\n", stderr: "diagnostic" });
  assert.equal(stdoutChunks[0], chunks[0]);
  assert.equal(stdoutChunks[1], chunks[1]);
  assert.equal(stderrChunks[0], chunks[2]);
});
