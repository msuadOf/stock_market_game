import assert from "node:assert/strict";
import test from "node:test";
import { SessionReplacementGate, synchronizeCurrentBaseline } from "./session-replacement.ts";

test("replacement operations serialize without sharing the file-picker epoch", () => {
  const gate = new SessionReplacementGate();
  const operation = gate.begin();
  assert.notEqual(operation, null);
  assert.equal(gate.begin(), null);
  let pickerEpoch = 0;
  pickerEpoch += 1;
  assert.equal(pickerEpoch, 1);
  assert.equal(gate.isCurrent(operation!), true);
  gate.finish(operation!);
  assert.notEqual(gate.begin(), null);
});

test("late rejected baseline sync is stale instead of failing a newer session", async () => {
  const gate = new SessionReplacementGate();
  const operation = gate.begin()!;
  let reject: (error: unknown) => void = () => { throw new Error("missing reject"); };
  const sync = new Promise<void>((_resolve, fail) => { reject = fail; });
  const pending = synchronizeCurrentBaseline(() => sync, () => gate.isCurrent(operation));
  gate.invalidate();
  const next = gate.begin()!;
  reject(new Error("old sync rejected"));
  assert.deepEqual(await pending, { kind: "stale" });
  assert.equal(gate.isCurrent(next), true);
});

test("current synchronization failure retains its actual cause and successful sync remains current", async () => {
  const error = new Error("authoritative host unavailable");
  assert.deepEqual(await synchronizeCurrentBaseline(async () => { throw error; }, () => true), { kind: "failed", error });
  assert.deepEqual(await synchronizeCurrentBaseline(async () => {}, () => true), { kind: "current" });
});
