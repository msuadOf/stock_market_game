import assert from "node:assert/strict";
import test from "node:test";
import { nextTauriGeneration, TauriTimelineState } from "./tauri-timeline-state.ts";

const snapshot = { seq: 4, tick: 4, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };

test("Tauri 三种 baseline 都携带对应会话的当前自然日", () => {
  const timeline = new TauriTimelineState();
  const initial = timeline.installInitialBaseline({ snapshot, generation: "1", timeline_id: "first", civil_date: "2030-01-02" });
  assert.equal(initial.civilDate, "2030-01-02");
  const refreshed = timeline.replaceRefreshedBaseline({ snapshot, generation: "1", timeline_id: "refreshed", civil_date: "2030-01-03" }, "1");
  assert.equal(refreshed.civilDate, "2030-01-03");
  const restored = timeline.replaceRestoredBaseline({ snapshot, generation: "2", timeline_id: "restored", civil_date: "2031-02-03" });
  assert.equal(restored.civilDate, "2031-02-03");
});

test("Tauri 拒绝无效自然日并保留旧 baseline，restore 仍切换到新 authority", () => {
  const timeline = new TauriTimelineState();
  const baseline = timeline.installInitialBaseline({ snapshot, generation: "1", timeline_id: "first", civil_date: "2030-01-02" });
  const cursor = timeline.captureQueryCursor();
  assert.throws(() => timeline.replaceRefreshedBaseline({ snapshot, generation: "1", timeline_id: "refreshed", civil_date: undefined }, "1"), /civil_date/);
  assert.equal(timeline.baselineForRead(), baseline);
  assert.doesNotThrow(() => timeline.assertQueryCursor(cursor, "1", "过期"));
  assert.throws(() => timeline.replaceRestoredBaseline({ snapshot, generation: "2", timeline_id: "restored", civil_date: "2030-02-30" }), /civil_date/);
  assert.equal(timeline.currentGeneration(), "2");
  assert.equal(timeline.matchesTimeline("restored"), true);
  assert.equal(timeline.baselineForRead(), baseline);
});

test("Tauri generation 在 Number.MAX_SAFE_INTEGER 之后仍精确递增", () => {
  assert.equal(nextTauriGeneration("9007199254740991"), "9007199254740992");
  assert.equal(nextTauriGeneration("9007199254740992"), "9007199254740993");
  assert.equal(nextTauriGeneration("18446744073709551615"), "18446744073709551616");
});

test("Tauri 刷新解析失败只切换 timeline，保持旧 baseline 和 query epoch", () => {
  const timeline = new TauriTimelineState();
  timeline.setInitialTimeline("session-1");
  assert.equal(timeline.matchesTimeline("session-1"), true);
  const baseline = timeline.installInitialBaseline({ snapshot, generation: "1", timeline_id: "timeline-1", civil_date: "2030-01-01" });
  const cursor = timeline.captureQueryCursor();
  assert.throws(() => timeline.replaceRefreshedBaseline({ snapshot: null, generation: "1", timeline_id: "timeline-failed", civil_date: "2030-01-01" }, "1"), /Tauri engine_baseline.snapshot/);
  assert.equal(timeline.matchesTimeline("timeline-failed"), true);
  assert.equal(timeline.baselineForRead(), baseline);
  assert.doesNotThrow(() => timeline.assertQueryCursor(cursor, "1", "过期"));
  timeline.replaceRefreshedBaseline({ snapshot, generation: "1", timeline_id: "timeline-refreshed", civil_date: "2030-01-01" }, "1");
  assert.throws(() => timeline.assertQueryCursor(cursor, "1", "过期"), /过期/);
  assert.doesNotThrow(() => timeline.assertGeneration("1", "过期"));
});

test("Tauri dispose 只清 timeline/cache，保留 generation/epoch 的既有查询检查", () => {
  const timeline = new TauriTimelineState();
  timeline.installInitialBaseline({ snapshot, generation: "1", timeline_id: "timeline-1", civil_date: "2030-01-01" });
  timeline.replaceRestoredBaseline({ snapshot, generation: "2", timeline_id: "timeline-2", civil_date: "2030-01-01" });
  const cursor = timeline.captureQueryCursor();
  timeline.clearForDispose();
  assert.equal(timeline.currentGeneration(), "2");
  assert.equal(timeline.matchesTimeline("timeline-2"), false);
  assert.equal(timeline.baselineForDelivery(), null);
  assert.throws(() => timeline.baselineForRead(), /Tauri 基线尚未就绪/);
  assert.doesNotThrow(() => timeline.assertQueryCursor(cursor, "2", "过期"));
  assert.throws(() => timeline.replaceRefreshedBaseline({ snapshot, generation: "1", timeline_id: "old", civil_date: "2030-01-01" }, "1"), /已过期会话 generation/);
});
