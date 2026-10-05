import assert from "node:assert/strict";
import test from "node:test";
import { chartSettingsReducer, setChartAverageSettings, setChartIndicator, toggleChartAverage, changeChartViewport, selectChartViewport } from "./chart-settings-slice.ts";

test("共用图表偏好独立于证券与视口，各股窗口分别保留", () => {
  const initial = chartSettingsReducer(undefined, { type: "init" });
  let state = chartSettingsReducer(initial, toggleChartAverage(5));
  state = chartSettingsReducer(state, setChartIndicator("macd"));
  state = chartSettingsReducer(state, changeChartViewport({ code: "600101", total: 361, action: "zoom-in" }));
  state = chartSettingsReducer(state, changeChartViewport({ code: "600101", total: 361, action: "pan-left" }));
  assert.deepEqual(state.selectedAverages, [10, 20, 30, 60]);
  assert.equal(state.indicator, "macd");
  assert.deepEqual(selectChartViewport(state, "600101"), { capacity: 48, offsetFromEnd: 12 });
  assert.deepEqual(selectChartViewport(state, "002156"), { capacity: 72, offsetFromEnd: 0 });
  state = chartSettingsReducer(state, changeChartViewport({ code: "002156", total: 361, action: "earliest" }));
  assert.deepEqual(selectChartViewport(state, "600101"), { capacity: 48, offsetFromEnd: 12 });
  assert.deepEqual(selectChartViewport(state, "002156"), { capacity: 72, offsetFromEnd: 289 });
  assert.equal(initial.viewports["600101"], undefined);
});

test("均线开关可恢复，窗口边界和复位沿用共用模型，未知均线不静默接受", () => {
  let state = chartSettingsReducer(undefined, toggleChartAverage(5));
  state = chartSettingsReducer(state, toggleChartAverage(5));
  assert.deepEqual([...state.selectedAverages].sort((a,b) => a-b), [5, 10, 20, 30, 60]);
  state = chartSettingsReducer(state, changeChartViewport({ code: "600101", total: 2, action: "pan-left" }));
  assert.deepEqual(selectChartViewport(state, "600101"), { capacity: 72, offsetFromEnd: 0 });
  state = chartSettingsReducer(state, changeChartViewport({ code: "600101", total: 361, action: "zoom-in" }));
  state = chartSettingsReducer(state, changeChartViewport({ code: "600101", total: 361, action: "reset" }));
  assert.deepEqual(selectChartViewport(state, "600101"), { capacity: 72, offsetFromEnd: 0 });
  assert.throws(() => chartSettingsReducer(state, toggleChartAverage(7)), /均线/);
  assert.equal(state.indicator, "kdj");
});

test("较短历史上首次右移从当前可见窗口起步，不重复夹回旧offset", () => {
  const earliest = chartSettingsReducer(undefined, changeChartViewport({ code: "600101", total: 361, action: "earliest" }));
  assert.equal(selectChartViewport(earliest, "600101").offsetFromEnd, 289);
  const moved = chartSettingsReducer(earliest, changeChartViewport({ code: "600101", total: 80, action: "pan-right" }));
  assert.deepEqual(selectChartViewport(moved, "600101"), { capacity: 72, offsetFromEnd: 0 });
});
test("可编辑MA周期及显示开关使用唯一Reduxowner，不改变证券viewport", { timeout: 10000 }, () => {
  let state = chartSettingsReducer(undefined, changeChartViewport({ code: "600101", total: 361, action: "zoom-in" }));
  state = chartSettingsReducer(state, setChartAverageSettings([{ period: 7, visible: true }, { period: 25, visible: false }]));
  assert.deepEqual(state.averagePeriods, [7, 25]);
  assert.deepEqual(state.selectedAverages, [7]);
  state = chartSettingsReducer(state, toggleChartAverage(25));
  assert.deepEqual(state.selectedAverages, [7, 25]);
  assert.deepEqual(selectChartViewport(state, "600101"), { capacity: 48, offsetFromEnd: 0 });
  assert.throws(() => chartSettingsReducer(state, setChartAverageSettings([{ period: 7, visible: true }, { period: 7, visible: false }])));
});
