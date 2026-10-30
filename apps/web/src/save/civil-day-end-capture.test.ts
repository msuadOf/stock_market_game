import assert from "node:assert/strict";
import test from "node:test";
import { captureCivilDayEndCandidate } from "./civil-day-end-capture.ts";

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((accept, fail) => { resolve = accept; reject = fail; });
  return { promise, resolve, reject };
}

test("日终候选未完成时不刷新挂单，当前候选完成后刷新一次", async () => {
  const pending = deferred<{ readonly seq: number }>();
  let current = true;
  let refreshes = 0;
  const captured = captureCivilDayEndCandidate(pending.promise, () => current, () => { refreshes += 1; });
  assert.equal(refreshes, 0);
  pending.resolve({ seq: 17 });
  assert.deepEqual(await captured, { seq: 17 });
  assert.equal(refreshes, 1);
});

test("日终候选失败保留拒绝且不刷新挂单", async () => {
  const pending = deferred<{ readonly seq: number }>();
  const failure = new Error("保存失败");
  let refreshes = 0;
  const captured = captureCivilDayEndCandidate(pending.promise, () => true, () => { refreshes += 1; });
  const rejected = assert.rejects(captured, (error) => error === failure);
  pending.reject(failure);
  await rejected;
  assert.equal(refreshes, 0);
});

test("等待日终候选期间宿主或 generation 变化时不刷新挂单", async () => {
  for (const replacement of ["host", "generation"]) {
    const pending = deferred<{ readonly seq: number }>();
    let current = true;
    let refreshes = 0;
    const captured = captureCivilDayEndCandidate(pending.promise, () => current, () => { refreshes += 1; });
    current = false;
    pending.resolve({ seq: 17 });
    await captured;
    assert.equal(refreshes, 0, `${replacement}变化后不得刷新`);
  }
});
