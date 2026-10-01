import assert from "node:assert/strict";
import { it } from "node:test";

import { main } from "./run-long-validation.mjs";

it("runs an explicitly classified long-validation command under an external deadline", async () => {
  await main(["1000", "--", process.execPath, "-e", "process.exit(0)"]);
});

it("rejects a long-validation deadline above five minutes", async () => {
  await assert.rejects(
    main(["300001", "--", process.execPath, "-e", "process.exit(0)"]),
    /within 300000ms/i,
  );
});

it("reserves one second inside the five-minute deadline for process-tree cleanup", async () => {
  const calls = [];
  await main(["300000", "--", process.execPath, "-e", "process.exit(0)"], { run: async (options) => calls.push(options) });
  assert.equal(calls.length, 1);
  assert.equal(calls[0].timeoutMs, 300000);
  assert.equal(calls[0].cleanupReserveMs, 1000);
});

it("keeps shorter diagnostic commands runnable with a proportional cleanup reserve", async () => {
  const calls = [];
  await main(["1000", "--", process.execPath, "-e", "process.exit(0)"], { run: async (options) => calls.push(options) });
  assert.equal(calls[0].timeoutMs, 1000);
  assert.equal(calls[0].cleanupReserveMs, 200);
});
