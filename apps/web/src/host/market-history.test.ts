import assert from "node:assert/strict";
import test from "node:test";
import { normalizeMarketHistoryPage, normalizeMarketHistoryRequest, wireMarketHistoryRequest } from "./market-history.ts";

// wire 的 stock 为数值 StockId；600101 ↔ id 3（STOCK_LIST 代码升序，ADR 0041）。
const request = { code: "600101", date_from: "2026-10-01", date_to: "2026-10-03", after: null, page_size: 2 };
const wireRequest = { stock: 3, date_from: "2026-10-01", date_to: "2026-10-03", after: null, page_size: 2 };
const bar = { minute_of_day: 565, phase: "OpenAuction", open: "100", high: "110", low: "90", close: "105", volume_shares: "18446744073709551615", trade_count: "2", turnover_cents: "1844674407370955161500" };
const wirePage = { stock: 3, entries: [{ date: "2026-10-01", availability: "Traded", bars: [bar], daily_candle: null }, { date: "2026-10-02", availability: "Closed", bars: [], daily_candle: null }], next_cursor: "2026-10-02", settled_through: "2026-10-02" };
const page = { code: "600101", entries: wirePage.entries, next_cursor: "2026-10-02", settled_through: "2026-10-02" };

test("永久分钟历史保留fullu64股数与u128金额，不转换为Number", { timeout: 10000 }, () => {
  assert.deepEqual(normalizeMarketHistoryRequest(request), request);
  assert.deepEqual(wireMarketHistoryRequest(request), wireRequest);
  assert.deepEqual(normalizeMarketHistoryPage(wirePage, request), page);
  // 请求侧翻译拒绝目录外代码（ADR 0041：未知代码显式抛错）。
  assert.throws(() => wireMarketHistoryRequest({ ...request, code: "600000" }), /未知股票代码/);
});

test("公开历史拒绝账户注入、缺字段、坏日期与游标", { timeout: 10000 }, () => {
  for (const item of [{ ...request, account: "1" }, { ...request, after: undefined }, { ...request, date_from: "2026-02-30" }, { ...request, after: "2026-09-30" }, { ...request, page_size: 0 }]) assert.throws(() => normalizeMarketHistoryRequest(item));
});

test("分钟历史拒绝假成交、数值金额、非规范数量和分页错配", { timeout: 10000 }, () => {
  for (const invalidBar of [{ ...bar, open: 100 }, { ...bar, volume_shares: "01" }, { ...bar, trade_count: "0" }, { ...bar, turnover_cents: "340282366920938463463374607431768211456" }, { ...bar, phase: "ClosingAuction", minute_of_day: 899 }]) assert.throws(() => normalizeMarketHistoryPage({ ...wirePage, entries: [{ ...wirePage.entries[0], bars: [invalidBar] }] }, request));
  assert.throws(() => normalizeMarketHistoryPage({ ...wirePage, stock: 4 }, request), /证券与请求不同/);
  assert.throws(() => normalizeMarketHistoryPage({ ...wirePage, stock: 99 }, request), /未知股票/);
  assert.throws(() => normalizeMarketHistoryPage({ ...wirePage, code: "600101" }, request), /字段不符合协议契约/);
  assert.throws(() => normalizeMarketHistoryPage({ ...wirePage, entries: [{ ...wirePage.entries[1], bars: [bar] }] }, request));
  assert.throws(() => normalizeMarketHistoryPage({ ...wirePage, entries: [...wirePage.entries].reverse() }, request));
  assert.throws(() => normalizeMarketHistoryPage({ ...wirePage, entries: [] }, request));
  assert.throws(() => normalizeMarketHistoryPage({ ...wirePage, next_cursor: null }, request));
});
