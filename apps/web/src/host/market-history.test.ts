import assert from "node:assert/strict";
import test from "node:test";
import { normalizeMarketHistoryPage, normalizeMarketHistoryRequest } from "./market-history.ts";

const request = { code: "600000", date_from: "2026-10-01", date_to: "2026-10-03", after: null, page_size: 2 };
const bar = { minute_of_day: 565, phase: "OpenAuction", open: "100", high: "110", low: "90", close: "105", volume_shares: "18446744073709551615", trade_count: "2", turnover_cents: "1844674407370955161500" };
const page = { code: "600000", entries: [{ date: "2026-10-01", availability: "Traded", bars: [bar], daily_candle: null }, { date: "2026-10-02", availability: "Closed", bars: [], daily_candle: null }], next_cursor: "2026-10-02", settled_through: "2026-10-02" };

test("永久分钟历史保留fullu64股数与u128金额，不转换为Number", { timeout: 10000 }, () => {
  assert.deepEqual(normalizeMarketHistoryRequest(request), request);
  assert.deepEqual(normalizeMarketHistoryPage(page, request), page);
});
test("公开历史拒绝账户注入、缺字段、坏日期与游标", { timeout: 10000 }, () => {
  for (const item of [{ ...request, account: "1" }, { ...request, after: undefined }, { ...request, date_from: "2026-02-30" }, { ...request, after: "2026-09-30" }, { ...request, page_size: 0 }]) assert.throws(() => normalizeMarketHistoryRequest(item));
});
test("分钟历史拒绝假成交、数值金额、非规范数量和分页错配", { timeout: 10000 }, () => {
  for (const invalidBar of [{ ...bar, open: 100 }, { ...bar, volume_shares: "01" }, { ...bar, trade_count: "0" }, { ...bar, turnover_cents: "340282366920938463463374607431768211456" }, { ...bar, phase: "ClosingAuction", minute_of_day: 899 }]) assert.throws(() => normalizeMarketHistoryPage({ ...page, entries: [{ ...page.entries[0], bars: [invalidBar] }] }, request));
  assert.throws(() => normalizeMarketHistoryPage({ ...page, code: "000001" }, request));
  assert.throws(() => normalizeMarketHistoryPage({ ...page, entries: [{ ...page.entries[1], bars: [bar] }] }, request));
  assert.throws(() => normalizeMarketHistoryPage({ ...page, entries: [...page.entries].reverse() }, request));
  assert.throws(() => normalizeMarketHistoryPage({ ...page, entries: [] }, request));
  assert.throws(() => normalizeMarketHistoryPage({ ...page, next_cursor: null }, request));
});
