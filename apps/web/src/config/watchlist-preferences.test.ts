import assert from "node:assert/strict";
import { test } from "node:test";
import { WatchlistPreferences, WATCHLIST_STORAGE_KEY } from "./watchlist-preferences.ts";

test("自选偏好区分未保存和明确清空，保留暂不在当前局的代码", () => {
  const values = new Map<string, string>();
  const repository = new WatchlistPreferences(() => ({ getItem: key => values.get(key) ?? null, setItem: (key, value) => { values.set(key, value); } }));
  assert.deepEqual(repository.load(), []);
  repository.save(["600101", "X0101"]);
  assert.deepEqual(repository.load(), ["600101", "X0101"]);
  repository.save([]);
  assert.equal(values.get(WATCHLIST_STORAGE_KEY), "[]");
  assert.deepEqual(repository.load(), []);
});

test("损坏的自选偏好和存储权限错误必须带上下文失败，不能假装空名单", () => {
  for (const raw of ["{", "null", "{}", '["600101", "600101"]', '[" 600101"]', '[""]', '[1]']) {
    const repository = new WatchlistPreferences(() => ({ getItem: () => raw, setItem: () => {} }));
    assert.throws(() => repository.load(), /读取自选失败/);
  }
  assert.throws(() => new WatchlistPreferences(() => { throw new Error("权限拒绝"); }).load(), /读取自选失败.*权限拒绝/);
  const repository = new WatchlistPreferences(() => ({ getItem: () => "[]", setItem: () => { throw new Error("空间不足"); } }));
  assert.throws(() => repository.save(["600101"]), /保存自选失败.*空间不足/);
});
