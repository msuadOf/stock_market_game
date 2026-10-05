import assert from "node:assert/strict";
import test from "node:test";
import { normalizeCurrentMinuteHistoryRequest, normalizeCurrentMinuteHistoryResponse } from "./current-minute-history.ts";

const request = { code: "600000" };
const bar = { minute_of_day: 565, phase: "OpenAuction", open: "100", high: "100", low: "100", close: "100", volume_shares: "100", trade_count: "1", turnover_cents: "10000" };
const response = { code: "600000", date: "2030-01-02", observed_at: { date: "2030-01-02", second_of_day: 36000 }, live: true, status: "Trading", phase: "Continuous", bars: [bar] };

test("当前分钟历史使用独立live响应，严格保留真实分钟事实", { timeout: 10000 }, () => {
  assert.deepEqual(normalizeCurrentMinuteHistoryRequest(request), request);
  assert.deepEqual(normalizeCurrentMinuteHistoryResponse(response, request), response);
  assert.deepEqual(normalizeCurrentMinuteHistoryResponse({ ...response, bars: [] }, request), { ...response, bars: [] });
});
test("当前分钟历史拒绝账户注入、归档冒充live与日期漂移", { timeout: 10000 }, () => {
  assert.throws(() => normalizeCurrentMinuteHistoryRequest({ ...request, account: "0" }));
  for (const invalid of [{ ...response, live: false }, { ...response, code: "000001" }, { ...response, observed_at: { date: "2030-01-03", second_of_day: 36000 } }, { ...response, status: "Closed", phase: "Closed" }, { ...response, bars: [bar, bar] }, { ...response, observed_at: { date: response.date, second_of_day: -1 } }]) assert.throws(() => normalizeCurrentMinuteHistoryResponse(invalid, request));
});
