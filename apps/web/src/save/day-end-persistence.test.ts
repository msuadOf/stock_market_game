import assert from "node:assert/strict";
import test from "node:test";
import * as persistence from "./day-end-persistence.ts";

test("day-end persistence writes only captured completed-day candidates", async () => {
  assert.equal(typeof persistence.DayEndPersistence, "function");
  const writes: unknown[] = [];
  const queue = new persistence.DayEndPersistence();
  queue.install("a");
  await queue.completed("a", Promise.resolve({ day: 1 }), async (slot) => { writes.push(slot); });
  assert.deepEqual(writes, [{ day: 1 }]);
});

test("day-end persistence rejects stale capture and serializes completed days", async () => {
  assert.equal(typeof persistence.DayEndPersistence, "function");
  const queue = new persistence.DayEndPersistence();
  const writes: unknown[] = [];
  queue.install("a");
  let resolve: (value: unknown) => void = () => { throw new Error("missing deferred resolve"); };
  const delayed = new Promise((done) => { resolve = done; });
  const old = queue.completed("a", delayed, async (slot) => { writes.push(slot); });
  queue.install("b");
  resolve({ day: 1 });
  assert.equal(await old, false);
  assert.equal(writes.length, 0);
  await queue.completed("b", Promise.resolve({ day: 2 }), async (slot, current) => {
    assert.equal(current(), true);
    writes.push(slot);
  });
  assert.deepEqual(writes, [{ day: 2 }]);
});

test("day-end persistence reports failure and allows the next completed day", async () => {
  assert.equal(typeof persistence.DayEndPersistence, "function");
  const queue = new persistence.DayEndPersistence();
  queue.install("a");
  await assert.rejects(queue.completed("a", Promise.resolve({ day: 1 }), async () => { throw new Error("disk full"); }), /disk full/);
  let saved: unknown;
  await queue.completed("a", Promise.resolve({ day: 2 }), async (slot) => { saved = slot; });
  assert.deepEqual(saved, { day: 2 });
});

test("day-end persistence invalidates an in-flight writer before its commit", async () => {
  assert.equal(typeof persistence.DayEndPersistence, "function");
  const queue = new persistence.DayEndPersistence();
  queue.install("a");
  let started: () => void = () => { throw new Error("missing start"); };
  const ready = new Promise<void>((resolve) => { started = resolve; });
  let finish: () => void = () => { throw new Error("missing finish"); };
  const delayed = new Promise<void>((resolve) => { finish = resolve; });
  let wrote = false;
  const pending = queue.completed("a", Promise.resolve({ day: 1 }), async (_slot, current) => {
    started();
    await delayed;
    if (current()) wrote = true;
  });
  await ready;
  queue.invalidate();
  finish();
  assert.equal(await pending, false);
  assert.equal(wrote, false);
});

test("day-end persistence preserves day order when captures resolve out of order", async () => {
  const queue = new persistence.DayEndPersistence();
  queue.install("a");
  const writes: unknown[] = [];
  let release: (value: unknown) => void = () => { throw new Error("missing release"); };
  const firstCapture = new Promise((resolve) => { release = resolve; });
  const first = queue.completed("a", firstCapture, async (slot) => { writes.push(slot); });
  const second = queue.completed("a", Promise.resolve({ day: 2 }), async (slot) => { writes.push(slot); });
  await Promise.resolve();
  assert.deepEqual(writes, []);
  release({ day: 1 });
  await Promise.all([first, second]);
  assert.deepEqual(writes, [{ day: 1 }, { day: 2 }]);
});

test("day-end persistence exposes capture failures without blocking later days", async () => {
  const queue = new persistence.DayEndPersistence();
  queue.install("a");
  await assert.rejects(queue.completed("a", Promise.reject(new Error("no day-end candidate")), async () => { assert.fail("must not write"); }), /no day-end candidate/);
  assert.equal(await queue.completed("a", Promise.resolve({ day: 2 }), async () => {}), true);
});

test("session replacement waits for a submitted atomic commit to finish", async () => {
  const queue = new persistence.DayEndPersistence();
  assert.equal(typeof queue.idle, "function");
  queue.install("a");
  let release: () => void = () => { throw new Error("missing release"); };
  const commit = new Promise<void>((resolve) => { release = resolve; });
  let entered: () => void = () => { throw new Error("missing entry"); };
  const started = new Promise<void>((resolve) => { entered = resolve; });
  const operation = queue.completed("a", Promise.resolve({ day: 1 }), async () => { entered(); await commit; });
  await started;
  queue.invalidate();
  let replaced = false;
  const replacement = queue.idle().then(() => { replaced = true; });
  await Promise.resolve();
  assert.equal(replaced, false);
  release();
  await Promise.all([operation, replacement]);
  assert.equal(replaced, true);
});

test("failed restore reinstalls the authoritative generation without reviving old captures", async () => {
  const queue = new persistence.DayEndPersistence();
  queue.install("a");
  let release: (value: unknown) => void = () => { throw new Error("missing release"); };
  const capture = new Promise((resolve) => { release = resolve; });
  let oldWrites = 0;
  const old = queue.completed("a", capture, async () => { oldWrites += 1; });
  queue.invalidate();
  queue.install("a");
  release({ day: 1 });
  assert.equal(await old, false);
  let newWrites = 0;
  assert.equal(await queue.completed("a", Promise.resolve({ day: 2 }), async () => { newWrites += 1; }), true);
  assert.equal(oldWrites, 0);
  assert.equal(newWrites, 1);
});
