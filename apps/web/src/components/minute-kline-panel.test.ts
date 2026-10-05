import assert from "node:assert/strict";
import { after, before, test } from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import type { MarketHistoryPage, MarketHistoryRequest } from "../host/market-history.ts";

let vite: ViteDevServer;
let Panel: typeof import("./MinuteKlinePanel.tsx").MinuteKlinePanel;
let MinuteKlineDay: typeof import("./MinuteKlinePanel.tsx").MinuteKlineDay;
let minuteVolumeBarHeight: typeof import("./MinuteKlinePanel.tsx").minuteVolumeBarHeight;
before(async () => {
  vite = await createServer({ configFile: false, appType: "custom", server: { middlewareMode: true, ws: false }, optimizeDeps: { noDiscovery: true } });
  const module = await vite.ssrLoadModule("/src/components/MinuteKlinePanel.tsx");
  Panel = module.MinuteKlinePanel;
  MinuteKlineDay = module.MinuteKlineDay;
  minuteVolumeBarHeight = module.minuteVolumeBarHeight;
});
after(async () => { await vite.close(); });

test("分钟K面板展示历史区间与选择周期，不声称活动日已归档", { timeout: 10000 }, () => {
  const query = async (_request: MarketHistoryRequest): Promise<MarketHistoryPage> => { throw new Error("SSR不发起宿主请求"); };
  const html = renderToStaticMarkup(React.createElement(Panel, { query, scopeKey: "1:0", code: "600001", currentDate: "2030-01-10", period: 15 }));
  assert.match(html, /15分钟历史K线/);
  assert.match(html, /分钟K线起始日期/);
  assert.match(html, /自然日和午间时段分别聚合/);
  assert.match(html, /当前交易日不在此历史页内/);
  assert.match(html, /2030-01-09/);
});

test("自然日尚未同步时仍显示查询表单且不伪造日期默认值", { timeout: 10000 }, () => {
  const query = async (_request: MarketHistoryRequest): Promise<MarketHistoryPage> => { throw new Error("SSR不发起宿主请求"); };
  const html = renderToStaticMarkup(React.createElement(Panel, { query, scopeKey: "1:0", code: "600001", currentDate: null, period: 120 }));
  assert.match(html, /120分钟历史K线/);
  assert.match(html, /value=""/);
});

type LiveResponse = { code: string; date: string; observed_at: { date: string; second_of_day: number }; live: true; status: "Trading" | "Closed"; phase: "CallAuction" | "PreOpen" | "Continuous" | "ClosingAuction" | "AfterClose" | "Closed"; bars: MarketHistoryPage["entries"][number]["bars"] };
function liveResponse(date: string, bars: LiveResponse["bars"] = []): LiveResponse {
  return { code: "600001", date, observed_at: { date, second_of_day: 36000 }, live: true, status: "Trading", phase: "Continuous", bars };
}
function mountLive() {
  const slots: { value: unknown; deps?: readonly unknown[] }[] = [];
  const effects: (() => void)[] = [];
  const cleanups = new Map<number, () => void>();
  const requests: { code: string; resolve: (value: LiveResponse) => void; reject: (error: Error) => void }[] = [];
  const historyQuery = async (_request: MarketHistoryRequest): Promise<MarketHistoryPage> => { throw new Error("history query not expected"); };
  const queryCurrentMinuteHistory = ({ code }: { code: string }) => new Promise<LiveResponse>((resolve, reject) => requests.push({ code, resolve, reject }));
  let currentDate = "2030-01-02", tickRefreshKey = 1, cursor = 0, tree: React.ReactNode;
  const internal = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  const dispatcher = {
    useState(initial: unknown) { const index = cursor++; if (slots[index] === undefined) slots[index] = { value: typeof initial === "function" ? (initial as () => unknown)() : initial }; return [slots[index].value, (value: unknown) => { slots[index].value = typeof value === "function" ? (value as (previous: unknown) => unknown)(slots[index].value) : value; }]; },
    useRef(initial: unknown) { const index = cursor++; if (slots[index] === undefined) slots[index] = { value: { current: initial } }; return slots[index].value; },
    useMemo(factory: () => unknown, deps: readonly unknown[]) { const index = cursor++; const slot = slots[index]; if (slot === undefined || slot.deps === undefined || deps.some((value, position) => !Object.is(value, slot.deps![position]))) slots[index] = { value: factory(), deps }; return slots[index].value; },
    useEffect(effect: () => void | (() => void), deps: readonly unknown[]) { const index = cursor++; const previous = slots[index]?.deps; if (previous === undefined || deps.some((value, position) => !Object.is(value, previous[position]))) { slots[index] = { value: undefined, deps }; effects.push(() => { cleanups.get(index)?.(); const cleanup = effect(); if (cleanup !== undefined) cleanups.set(index, cleanup); }); } },
  };
  function render() {
    const previous = internal.ReactCurrentDispatcher.current;
    cursor = 0; internal.ReactCurrentDispatcher.current = dispatcher;
    try { tree = Panel({ query: historyQuery, queryCurrentMinuteHistory, tickRefreshKey, scopeKey: "market:1", code: "600001", currentDate, period: 5 }); } finally { internal.ReactCurrentDispatcher.current = previous; }
    while (effects.length) effects.shift()!();
  }
  render(); render();
  return { requests, render, content: () => renderToStaticMarkup(tree), changeDate(date: string) { currentDate = date; tickRefreshKey += 1; render(); render(); }, dispose() { cleanups.forEach((cleanup) => cleanup()); } };
}
async function settleLive() { for (let index = 0; index < 8; index++) await Promise.resolve(); }

test("实时分钟使用真实空bar、不合成K线，切换日期丢弃迟到响应且失败不显示旧日", { timeout: 10000 }, async () => {
  const ui = mountLive();
  try {
    assert.equal(ui.requests.length, 1);
    ui.changeDate("2030-01-03");
    assert.equal(ui.requests.length, 2);
    ui.requests[0].resolve(liveResponse("2030-01-02"));
    await settleLive(); ui.render();
    assert.doesNotMatch(ui.content(), /2030-01-02（实时）/);
    ui.requests[1].reject(new Error("当前查询失败"));
    await settleLive(); ui.render();
    assert.match(ui.content(), /当前查询失败/);
    assert.doesNotMatch(ui.content(), /2030-01-02（实时）/);
  } finally { ui.dispose(); }
});

test("当前分钟查询返回空成交列表时不生成分时或K线柱", { timeout: 10000 }, async () => {
  const ui = mountLive();
  try {
    ui.requests[0].resolve(liveResponse("2030-01-02", []));
    await settleLive(); ui.render();
    assert.match(ui.content(), /当前尚无真实分钟成交，不生成K线柱/);
    assert.doesNotMatch(ui.content(), /<svg/);
  } finally { ui.dispose(); }
});

test("当前分钟宿主返回其他自然日时显式拒绝", { timeout: 10000 }, async () => {
  const ui = mountLive();
  try {
    ui.requests[0].resolve(liveResponse("2030-01-01"));
    await settleLive(); ui.render();
    assert.match(ui.content(), /自然日错配/);
    assert.doesNotMatch(ui.content(), /2030-01-01（实时）/);
  } finally { ui.dispose(); }
});

test("分钟K价格与成交量共享主图坐标布局并遵守红涨空心、绿跌实心", { timeout: 10000 }, () => {
  const bars = [
    { date: "2030-01-02", minute_of_day: 570, phase: "Continuous" as const, open: "100", high: "110", low: "90", close: "105", volume_shares: "100", turnover_cents: "10250", trade_count: "2" },
    { date: "2030-01-02", minute_of_day: 575, phase: "Continuous" as const, open: "105", high: "106", low: "95", close: "98", volume_shares: "200", turnover_cents: "20100", trade_count: "3" },
  ];
  const html = renderToStaticMarkup(React.createElement(MinuteKlineDay, { date: "2030-01-02", bars, period: 5 }));
  assert.match(html, /分钟K价格坐标，单位为元/);
  assert.match(html, /分钟K成交量坐标，单位为股/);
  assert.match(html, /09:30/);
  assert.match(html, /<g class="rise"/);
  assert.match(html, /<g class="fall"/);
  assert.match(html, /class="upper-wick"/);
  assert.match(html, /class="lower-wick"/);
  assert.match(html, /class="rise"[^>]*>/);
  assert.match(html, /分钟K成交量/);
});

test("分钟量柱极小股数仍显示至少一像素且最大值保持满高", { timeout: 10000 }, () => {
  const maximum = 18446744073709551615n;
  assert.equal(minuteVolumeBarHeight(1n, maximum), 1);
  assert.equal(minuteVolumeBarHeight(maximum, maximum), 62);
  assert.equal(minuteVolumeBarHeight(0n, maximum), 0);
  const bars = [
    { date: "2030-01-02", minute_of_day: 570, phase: "Continuous" as const, open: "9223372036854775807", high: "9223372036854775807", low: "9223372036854775806", close: "9223372036854775806", volume_shares: "18446744073709551615", turnover_cents: "170141183460469231704017187605319778305", trade_count: "1" },
    { date: "2030-01-02", minute_of_day: 571, phase: "Continuous" as const, open: "9223372036854775807", high: "9223372036854775807", low: "9223372036854775806", close: "9223372036854775806", volume_shares: "1", turnover_cents: "9223372036854775806", trade_count: "1" },
  ];
  const html = renderToStaticMarkup(React.createElement(MinuteKlineDay, { date: "2030-01-02", bars, period: 1 }));
  assert.match(html, /<g class="fall"/);
  assert.match(html, /class="fall"[^>]*x="[^\"]+" y="69" width="4" height="1"/);
  assert.match(html, /class="fall"[^>]*x="[^\"]+" y="8" width="4" height="62"/);
});
