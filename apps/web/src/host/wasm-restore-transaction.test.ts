import assert from "node:assert/strict";
import test from "node:test";
import { restoreWasmSession } from "./wasm-restore-transaction.ts";

for (const boundary of ["direct host", "worker"] as const) {
  test(`${boundary} keeps the old running session when restored snapshot validation fails`, () => {
    let handle = 7;
    let running = true;
    const dropped: number[] = [];

    assert.throws(
      () => restoreWasmSession({
        currentHandle: () => handle,
        replaceHandle: (next) => { handle = next; },
        restore: () => 9,
        snapshot: () => { throw new Error("invalid restored snapshot"); },
        drop: (target) => { dropped.push(target); },
        wasRunning: running,
        stop: () => { running = false; },
        restart: () => { running = true; },
      }),
      /invalid restored snapshot/,
    );

    assert.equal(handle, 7);
    assert.equal(running, true);
    assert.deepEqual(dropped, [9]);
  });
}

test("restore swaps authority only after the candidate snapshot succeeds", () => {
  let handle = 7;
  const dropped: number[] = [];
  const snapshot = restoreWasmSession({
    currentHandle: () => handle,
    replaceHandle: (next) => { handle = next; },
    restore: () => 9,
    snapshot: (target) => ({ target }),
    drop: (target) => { dropped.push(target); },
    wasRunning: false,
    stop: () => assert.fail("stopped a paused session"),
    restart: () => assert.fail("restarted a paused session"),
  });

  assert.deepEqual(snapshot, { target: 9 });
  assert.equal(handle, 9);
  assert.deepEqual(dropped, [7]);
});
