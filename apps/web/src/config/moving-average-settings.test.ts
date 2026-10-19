import assert from "node:assert/strict";
import { test } from "node:test";
import { DEFAULT_MOVING_AVERAGES, loadMovingAverageSettings, parseMovingAverageSettings, saveMovingAverageSettings } from "./moving-average-settings.ts";
test("显示偏好默认MA5/10/20/30/60，保存编辑及开关后读取完全一致", { timeout: 10000 }, () => {
  let stored: string | null = null;
  const storage = { getItem: () => stored, setItem: (_key: string, value: string) => { stored = value; } };
  assert.deepEqual(loadMovingAverageSettings(storage), DEFAULT_MOVING_AVERAGES);
  const edited = [{ period: 3, visible: false }, { period: 25, visible: true }];
  saveMovingAverageSettings(storage, edited);
  assert.deepEqual(loadMovingAverageSettings(storage), edited);
});
test("设置严格拒绝非正/小数/重复周期及缺少/额外字段，不修补旧格式", { timeout: 10000 }, () => {
  for (const value of [null, {}, [{ period: 0, visible: true }], [{ period: 1.5, visible: true }], [{ period: 3, visible: true }, { period: 3, visible: false }], [{ period: 5 }], [{ period: "5", visible: true }], [{ period: 5, visible: true, extra: 1 }]]) assert.throws(() => parseMovingAverageSettings(value));
  assert.deepEqual(parseMovingAverageSettings([]), []);
});
test("保存失败显式传播且不改已有持久偏好；损坏读取不能回退默认", { timeout: 10000 }, () => {
  assert.throws(() => saveMovingAverageSettings({ getItem: () => "[]", setItem: () => { throw new Error("空间不足"); } }, DEFAULT_MOVING_AVERAGES), /空间不足/);
  assert.throws(() => loadMovingAverageSettings({ getItem: () => "invalid", setItem: () => {} }));
});
