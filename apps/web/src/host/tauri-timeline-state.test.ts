import assert from "node:assert/strict";
import test from "node:test";
import { nextTauriGeneration, TauriTimelineState } from "./tauri-timeline-state.ts";

const snapshot = { seq: 4, tick: 4, day: 0, phase: "Continuous", markets: {}, accounts: {}, daily_candles: {}, active_daily_candles: {} };

test("Tauri generation 在 Number.MAX_SAFE_INTEGER 之后仍精确递增", () => {
  assert.equal(nextTauriGeneration("9007199254740991"), "9007199254740992");
  assert.equal(nextTauriGeneration("9007199254740992"), "9007199254740993");
  assert.equal(nextTauriGeneration("18446744073709551615"), "18446744073709551616");
});

test("Tauri 刷新解析失败只切换 timeline，保持旧 baseline 和 query epoch", () => {
  const timeline = new TauriTimelineState();
  timeline.setInitialTimeline("session-1");
  assert.equal(timeline.matchesTimeline("session-1"), true);
  const baseline = timeline.installInitialBaseline({ snapshot, generation: "1", timeline_id: "timeline-1" });
  const cursor = timeline.captureQueryCursor();
  assert.throws(() => timeline.replaceRefreshedBaseline({ snapshot: null, generation: "1", timeline_id: "timeline-failed" }, "1"), /Tauri engine_baseline.snapshot/);
  assert.equal(timeline.matchesTimeline("timeline-failed"), true);
  assert.equal(timeline.baselineForRead(), baseline);
  assert.doesNotThrow(() => timeline.assertQueryCursor(cursor, "1", "过期"));
  timeline.replaceRefreshedBaseline({ snapshot, generation: "1", timeline_id: "timeline-refreshed" }, "1");
  assert.throws(() => timeline.assertQueryCursor(cursor, "1", "过期"), /过期/);
  assert.doesNotThrow(() => timeline.assertGeneration("1", "过期"));
});

test("Tauri dispose 只清 timeline/cache，保留 generation/epoch 的既有查询检查", () => {
  const timeline = new TauriTimelineState();
  timeline.installInitialBaseline({ snapshot, generation: "1", timeline_id: "timeline-1" });
  timeline.replaceRestoredBaseline({ snapshot, generation: "2", timeline_id: "timeline-2" });
  const cursor = timeline.captureQueryCursor();
  timeline.clearForDispose();
  assert.equal(timeline.currentGeneration(), "2");
  assert.equal(timeline.matchesTimeline("timeline-2"), false);
  assert.equal(timeline.baselineForDelivery(), null);
  assert.throws(() => timeline.baselineForRead(), /Tauri 基线尚未就绪/);
  assert.doesNotThrow(() => timeline.assertQueryCursor(cursor, "2", "过期"));
  assert.throws(() => timeline.replaceRefreshedBaseline({ snapshot, generation: "1", timeline_id: "old" }, "1"), /已过期会话 generation/);
});
