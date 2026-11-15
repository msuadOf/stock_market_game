// 实际 Tauri WebView 中，直接消费现有 IPC：可测同一 fixture 的权威与送达，不把它当 React UI 吞吐。
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { DEFAULT_SETUP } from "../../../apps/web/src/config/defaults.ts";
import { makeSetup, SCENARIO } from "../../../scripts/fixtures/libos-product-scenario.mjs";
import { connectWebKitInspector, waitFor } from "./webkit-inspector.mjs";

const [port, mode] = process.argv.slice(2);
assert.ok(["stalled", "fixed", "max"].includes(mode), "用法：desktop-controlled.mjs <Inspector 端口> <stalled|fixed|max>");
const setup = makeSetup(DEFAULT_SETUP);
const dataset = { setup, seed: SCENARIO.seed, code: SCENARIO.codes[0], startDate: SCENARIO.startDate, mode };
const setupSha256 = createHash("sha256").update(JSON.stringify(dataset.setup)).digest("hex");

async function inWebView(input) {
  const { setup: gameSetup, seed, mode: selected, code, startDate } = input;
  const tauri = globalThis.__TAURI_INTERNALS__;
  if (typeof tauri?.invoke !== "function" || typeof tauri?.transformCallback !== "function") throw new Error("没有实际 Tauri IPC");
  const invoke = (name, data) => tauri.invoke(name, data);
  const sleep = (milliseconds) => new Promise((resolve) => setTimeout(resolve, milliseconds));
  const generation = "1";
  const listeners = [];
  let sessionId;
  let ordinalCount = 0, observedTick = 0, deliveredTicks = 0, deliveredFacts = 0, gapCount = 0;
  let acknowledged = 0;
  let error = null;
  let ack = Promise.resolve();
  const watch = async (event, callback) => {
    const handler = tauri.transformCallback(({ payload }) => {
      if (sessionId !== undefined && payload.session_id === sessionId) callback(payload);
    });
    const eventId = await invoke("plugin:event|listen", { event, target: { kind: "Any" }, handler });
    listeners.push({ event, eventId });
  };
  const cleanup = async () => {
    const operations = [];
    if (sessionId !== undefined) operations.push(invoke("stop_session", { sessionId }));
    for (const { event, eventId } of listeners) {
      globalThis.__TAURI_EVENT_PLUGIN_INTERNALS__.unregisterListener(event, eventId);
      operations.push(invoke("plugin:event|unlisten", { event, eventId }));
    }
    const settled = await Promise.allSettled(operations);
    const errors = settled.filter((item) => item.status === "rejected").map((item) => String(item.reason));
    if (errors.length > 0) throw new Error("Desktop fixture 清理失败：" + errors.join("; "));
  };
  const requireHealthy = () => {
    if (error !== null) throw new Error("Desktop fixture 观察到宿主故障：" + error);
  };
  const state = () => invoke("snapshot", { sessionId });
  const metrics = () => invoke("speed_metrics", { sessionId });
  const history = async (kind, data) => {
    const response = await invoke("host_history_query", { sessionId, generation, query: { kind, data } });
    if (response.generation !== generation) throw new Error("Desktop fixture 世代意外改变");
    return response.value;
  };
  const main = async () => {
    await watch("engine-event", (payload) => {
      if (payload.gap !== undefined) {
        gapCount++;
        return;
      }
      const { update, publication_ordinal: ordinal } = payload;
      if (typeof ordinal !== "string") { error = "市场增量未使用有序发布"; return; }
      const frames = update.TickBatch?.frames ?? [];
      ordinalCount++;
      if (frames.length > 0) {
        observedTick = frames.at(-1).tick;
        deliveredTicks += frames.length;
        deliveredFacts += frames.reduce((sum, frame) => sum + frame.facts.length, 0);
      } else if (update.CivilUpdate !== undefined) observedTick = update.CivilUpdate.tick;
      else { error = "Desktop fixture 收到未知的发布消息"; return; }
      if (selected !== "stalled") {
        ack = ack.then(async () => {
          await invoke("engine_ack_observer", { sessionId, generation, ordinal });
          acknowledged++;
        }).catch((failure) => { error = String(failure); });
      }
    });
    await watch("engine-failure", (payload) => {
      error = JSON.stringify(payload);
    });
    sessionId = await invoke("create_session", { setup: gameSetup, seed });
    const baseline = await invoke("engine_baseline", { sessionId, generation });
    const stockInfo = baseline.stock_catalog.find((entry) => entry.code === code);
    if (stockInfo === undefined || baseline.snapshot.tick !== 0) throw new Error("Desktop fixture 初始基线不匹配");
    await invoke("set_pause_preferences", { sessionId, preferences: { pause_after_close: false, pause_before_open: false } });
    const speed = selected === "max" ? "Fastest" : { Fixed: selected === "fixed" ? 1 : 12 };
    await invoke("set_speed", { sessionId, speed });
    await invoke("resume_session", { sessionId });
    if (selected === "stalled") {
      let priorMinuteCoordinates = [];
      let next;
      const limit = performance.now() + 28_000;
      while (performance.now() < limit) {
        requireHealthy();
        const day = await history("CurrentMinuteHistory", { stock: stockInfo.id });
        if (day.date === startDate && day.bars.length >= priorMinuteCoordinates.length) priorMinuteCoordinates = day.bars.map(({ minute_of_day }) => minute_of_day);
        if (day.date > startDate && day.trading && day.phase === "Continuous") { next = day; break; }
        await sleep(35);
      }
      if (next === undefined) throw new Error("无 ACK 时 Desktop 未在期限内跨交易日");
      await invoke("pause_session", { sessionId });
      const closed = (await history("MarketHistory", { stock: stockInfo.id, date_from: startDate, date_to: startDate, after: null, page_size: 1 })).entries[0];
      return {
        startedTick: baseline.snapshot.tick, observedTick, authorityTick: (await state()).tick, ordinalCount, acknowledged, gapCount,
        nextDate: next.date, nextMinutes: next.bars.map(({ minute_of_day }) => minute_of_day), priorMinuteCoordinates,
        previous: { bars: closed.bars.length, availability: closed.availability, volume: closed.daily_candle?.volume ?? 0, time: closed.daily_candle?.time },
      };
    }
    await sleep(1_100);
    const begin = await state();
    const start = performance.now();
    const counts = { deliveredTicks, deliveredFacts, gapCount, acknowledged };
    const samples = [];
    for (let index = 0; index < 3; index++) {
      await sleep(850);
      requireHealthy();
      const sample = await metrics();
      if (!sample.running || (selected === "max" && sample.sample_ticks === 0) || sample.sample_duration_ms < 500) throw new Error("Desktop 未提供完整运行采样窗：" + JSON.stringify(sample));
      samples.push(sample);
    }
    const after = await state();
    const wallMs = performance.now() - start;
    const endCounts = { deliveredTicks, deliveredFacts, gapCount, acknowledged };
    await invoke("pause_session", { sessionId });
    await ack;
    requireHealthy();
    return {
      window: { wallMs, tickBefore: begin.tick, seqBefore: begin.seq, tickAfter: after.tick, seqAfter: after.seq },
      authorityTicksPerSecond: (after.tick - begin.tick) * 1_000 / wallMs,
      deliveredTicks: endCounts.deliveredTicks - counts.deliveredTicks,
      deliveredFacts: endCounts.deliveredFacts - counts.deliveredFacts,
      gapCount: endCounts.gapCount - counts.gapCount, acknowledged: endCounts.acknowledged - counts.acknowledged, samples,
    };
  };
  let result, failure;
  try { result = await main(); } catch (exception) { failure = exception; }
  try { await cleanup(); } catch (exception) {
    if (failure !== undefined) throw new AggregateError([failure, exception], "Desktop fixture 运行与清理都失败");
    throw exception;
  }
  if (failure !== undefined) throw failure;
  return result;
}

const app = await connectWebKitInspector(Number(port));
try {
  await waitFor("实际 Tauri 命令启动", () => app.evaluate("typeof globalThis.__TAURI_INTERNALS__?.invoke === 'function'"), 15_000);
  await app.evaluate(`(() => {
    globalThis.__desktopControlled = { status: "running" };
    const run = (${inWebView.toString()});
    void run(${JSON.stringify(dataset)}).then(
      (result) => { globalThis.__desktopControlled = { status: "passed", result }; },
      (error) => { globalThis.__desktopControlled = { status: "failed", error: String(error), detail: JSON.stringify(error?.errors ?? null) }; },
    );
    return true;
  })()`);
  const report = await waitFor("Desktop 受控测试结束", async () => {
    const result = await app.evaluate("globalThis.__desktopControlled");
    return result.status === "running" ? undefined : result;
  }, 42_000);
  assert.equal(report.status, "passed", report.error + " " + report.detail);
  console.log(JSON.stringify({ mode, seed: dataset.seed, setupSha256, ...report.result }));
  if (mode === "stalled") {
    const run = report.result;
    assert.equal(run.ordinalCount, 2, "必须耗尽真实发送窗口");
    assert.equal(run.acknowledged, 0);
    assert.equal(run.nextDate, "2030-01-07");
    assert.ok(run.observedTick < SCENARIO.ticksPerDay && run.authorityTick > SCENARIO.ticksPerDay);
    assert.ok(run.priorMinuteCoordinates.some(minute => minute >= 13 * 60));
    assert.ok(run.nextMinutes.every(minute => minute < 12 * 60));
    assert.equal(run.previous.bars, 0);
    assert.equal(run.previous.availability, "Traded");
    assert.ok(run.previous.volume > 0 && run.previous.time === Date.parse(dataset.startDate) / 1000);
  }
} finally {
  app.close();
}
