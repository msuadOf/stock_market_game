import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { HostUpdate } from "../host/host-update.ts";
import { createBaselineUpdate, createProtocolUpdate } from "../host/host-update.ts";
import { parseEngineUpdate, parseProtocolSnapshot } from "../host/protocol/parse.ts";
import { canonicalJson } from "../host/protocol/canonical.ts";
import { civilUpdate, dailyCandle, frame, market, snapshot, timeseries } from "../host/protocol-test-fixtures.ts";
import type { ChartRuntimeObservation } from "./market-chart-runtime.test-support.tsx";

let vite: ViteDevServer;
let probe: typeof import("./market-chart-runtime.test-support.tsx");

before(async () => {
  vite = await createServer({
    configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true },
  });
  probe = await vite.ssrLoadModule("/src/app/market-chart-runtime.test-support.tsx") as typeof probe;
});

after(async () => { if (vite) await vite.close(); });

function baseline() {
  return createBaselineUpdate("1", parseProtocolSnapshot({
    ...snapshot(0, 0), markets: { "600101": market(), "000001": market() },
    accounts: { "0": { cash: "100000", reserved_cash: "0", positions: {}, reserved_sell_qty: {} } },
    daily_candles: { "600101": [{ ...dailyCandle(), time: -1, open: "900", high: "900", low: "900", close: "900" }], "000001": [] },
    active_daily_candles: { "600101": dailyCandle(), "000001": dailyCandle() },
  }));
}

function update(active: unknown, tick = 1, cursor = 0): Extract<HostUpdate, { type: "protocol" }> {
  return createProtocolUpdate("1", { TickBatch: {
    frames: [{ ...frame(tick, cursor, []), timeseries_payload: {
      ...timeseries(tick), markets: { "600101": market(), "000001": market() },
      active_daily_candles: active, continuous_points: {},
    } }],
    runtime_snapshot: null,
    runtime_delta: {
      tick, seq_from: cursor, seq_to: cursor, day: 0, phase: "Continuous", accounts: {},
      working_orders: { reset: tick === 1, upserts: [], removed: [] },
    },
  } });
}

function render(updates: readonly Extract<HostUpdate, { type: "protocol" }>[], options: {
  baseline?: Extract<HostUpdate, { type: "baseline" }>;
  afterUpdates?: NonNullable<Parameters<typeof probe.MarketChartRuntimeProbe>[0]["afterUpdates"]>;
} = {}) {
  const observed: Partial<Record<"baseline" | "updated", ChartRuntimeObservation>> = {};
  renderToStaticMarkup(createElement(probe.MarketChartRuntimeProbe, {
    baseline: options.baseline ?? baseline(), updates, afterUpdates: options.afterUpdates,
    observe: (stage, value) => { observed[stage] = value; },
  }));
  assert.ok(observed.baseline);
  assert.ok(observed.updated);
  return { before: observed.baseline, after: observed.updated };
}

test("production delta updates selected daily K and mobile active K cache while retaining completed history", () => {
  const active = { ...dailyCandle(), high: "1250", close: "1200", volume: 250, trade_stats: { turnover_cents: "300000", trade_count: 2 } };
  const { before, after } = render([update({ "600101": active, "000001": { ...active, close: "1100" } })]);
  assert.equal(after.active["600101"]!.close, 12);
  assert.equal(after.active["000001"]!.close, 11);
  assert.deepEqual(after.daily.at(-1), after.active["600101"]);
  assert.equal(after.daily.length, 2);
  assert.equal(after.daily[0], before.daily[0]);
  assert.equal(after.active["600101"]!.volume, 250);
  assert.deepEqual(after.active["600101"]!.tradeStats, { turnoverCents: "300000", tradeCount: 2 });
});

test("production runtimeSnapshot 替换权威日 K，普通帧的 active 不覆盖 snapshot", { timeout: 10000 }, () => {
  const authoritative = { ...baseline().snapshot, tick: 1,
    daily_candles: { "600101": [{ ...dailyCandle(), time: -2, close: "800" }] },
    active_daily_candles: { "600101": { ...dailyCandle(), close: "1400" } },
  };
  const { after } = render([createProtocolUpdate("1", { TickBatch: {
    frames: [{ ...frame(1, 0, []), timeseries_payload: { ...timeseries(1),
      continuous_points: {}, active_daily_candles: { "600101": { ...dailyCandle(), close: "1200" } },
    } }], runtime_snapshot: authoritative,
  } })]);
  assert.equal(after.daily.length, 2);
  assert.equal(after.daily[0].close, 8);
  assert.equal(after.daily.at(-1)!.close, 14);
  assert.equal(after.active["600101"]!.close, 14);
});

test("production AfterClose+BeforeOpen 即使携旧日历史也清空分时并保留权威日 K", { timeout: 10000 }, () => {
  const initial = createBaselineUpdate("1", parseProtocolSnapshot(snapshot(0, 0)));
  const first = createProtocolUpdate("1", { TickBatch: {
    frames: [{ ...frame(1, 0, ["600000"]), timeseries_payload: {
      ...timeseries(1), continuous_points: { old: { tick: 1, phase: "Continuous", last_price: "1000", cumulative_volume: 100, bids: [], asks: [] } },
    } }], runtime_snapshot: snapshot(1, 1),
  } });
  const civil = createProtocolUpdate("1", civilUpdate());
  const selected = render([first, civil], { baseline: initial, afterUpdates: (runtime) => runtime.selectChart("600000") }).after;
  assert.equal(selected.code, "600000");
  assert.equal(Object.hasOwn(selected.history, "old"), false);
  assert.deepEqual(selected.prices, []);
  assert.deepEqual(selected.auctions, []);
  assert.deepEqual(selected.history, {});
  assert.equal(selected.daily.length, 1);
  assert.equal(selected.active["600000"]!.close, 10);
  const reset = render([first, civil], { baseline: initial, afterUpdates: (runtime) => runtime.resetMarketHistory(initial.snapshot) }).after;
  assert.deepEqual(reset.history, {});
  assert.deepEqual(reset.prices, []);
  assert.deepEqual(reset.auctions, []);
  assert.equal(reset.active["600000"]!.close, 10);
});

test("successive production deltas use final authoritative active candles and remove absent active codes", () => {
  const { before, after } = render([
    update({ "600101": { ...dailyCandle(), close: "1200" }, "000001": dailyCandle() }),
    update({ "600101": { ...dailyCandle(), high: "1400", close: "1300", volume: 300 } }, 2),
  ]);
  assert.equal(after.daily.at(-1)!.close, 13);
  assert.equal(after.active["600101"]!.volume, 300);
  assert.equal(Object.hasOwn(after.active, "000001"), false);
  assert.equal(after.daily[0], before.daily[0]);
});

test("production AfterClose-only 进入非交易日时保留真实收盘分时", { timeout: 10000 }, () => {
  const initial = createBaselineUpdate("1", parseProtocolSnapshot(snapshot(1, 1)));
  const parsed = parseEngineUpdate(civilUpdate());
  if (!("CivilUpdate" in parsed)) throw new Error("CivilUpdate fixture 类型错误");
  const event = { CivilDateAdvanced: {
    seq: 2, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: { Closed: "Weekend" as const },
  } };
  const civil = createProtocolUpdate("1", { CivilUpdate: { ...parsed.CivilUpdate,
    kinds: ["AfterClose"], boundary: { ...parsed.CivilUpdate.boundary, next_status: { Closed: "Weekend" } },
    events: [event], facts: [{ ...parsed.CivilUpdate.facts[0], event, canonical_payload: canonicalJson(event) }],
  } });
  const after = render([civil], { baseline: initial, afterUpdates: (runtime) => runtime.selectChart("600000") }).after;
  assert.equal(after.prices.length, 1);
  assert.equal(after.prices[0].volume, 1);
  assert.equal(after.prices[0].value, 10);
  assert.equal(after.daily.length, 1);
});
