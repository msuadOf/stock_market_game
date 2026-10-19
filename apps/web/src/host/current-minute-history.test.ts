import assert from "node:assert/strict";
import test from "node:test";
import { normalizeCurrentMinuteHistoryRequest, normalizeCurrentMinuteHistoryResponse, wireCurrentMinuteHistoryRequest } from "./current-minute-history.ts";

// wire 的 stock 为数值 StockId；000812 ↔ id 0（STOCK_LIST 代码升序，ADR 0041）。
const request = { code: "000812" };
const bar = { minute_of_day: 565, phase: "OpenAuction", open: "100", high: "100", low: "100", close: "100", volume_shares: "100", trade_count: "1", turnover_cents: "10000" };
// B2 wire：stock 数值 + trading 布尔（旧 status/observed_at 不再下发）。
const wireResponse = { stock: 0, date: "2030-01-02", live: true, trading: true, phase: "Continuous", bars: [bar] };
const response = { code: "000812", date: "2030-01-02", live: true, trading: true, phase: "Continuous", bars: [bar] };

test("当前分钟历史使用独立live响应，严格保留真实分钟事实", { timeout: 10000 }, () => {
  assert.deepEqual(normalizeCurrentMinuteHistoryRequest(request), request);
  assert.deepEqual(wireCurrentMinuteHistoryRequest(request), { stock: 0 });
  assert.deepEqual(normalizeCurrentMinuteHistoryResponse(wireResponse, request), response);
  assert.deepEqual(normalizeCurrentMinuteHistoryResponse({ ...wireResponse, bars: [] }, request), { ...response, bars: [] });
});
test("当前分钟历史拒绝账户注入、归档冒充live与证券错配", { timeout: 10000 }, () => {
  assert.throws(() => normalizeCurrentMinuteHistoryRequest({ ...request, account: "0" }));
  for (const invalid of [{ ...wireResponse, live: false }, { ...wireResponse, stock: 1 }, { ...wireResponse, trading: false, phase: "Continuous" }, { ...wireResponse, trading: false, bars: [bar] }, { ...wireResponse, phase: "Closed" }, { ...wireResponse, bars: [bar, bar] }, { ...wireResponse, trading: "yes" }]) assert.throws(() => normalizeCurrentMinuteHistoryResponse(invalid, request));
});
