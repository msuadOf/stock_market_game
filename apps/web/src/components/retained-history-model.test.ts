import assert from "node:assert/strict";
import { test } from "node:test";
import { latestFiveTradingDays } from "./retained-history-model.ts";

test("五日读取越过休市但保留零成交，不把当前日或虚拟前史当分钟历史", { timeout: 10000 }, async () => {
  const requests: { date_to: string }[] = [];
  const page = await latestFiveTradingDays(async (request) => {
    requests.push(request);
    const dates = request.date_to === "2030-01-09" ? ["2030-01-03", "2030-01-04", "2030-01-05", "2030-01-06", "2030-01-07", "2030-01-08", "2030-01-09"] : ["2029-12-27", "2029-12-28", "2029-12-29", "2029-12-30", "2029-12-31", "2030-01-01", "2030-01-02"];
    return { code: request.code, entries: dates.map((date) => ({ date, availability: date < "2030-01-02" ? "BeforeStart" : ["2030-01-05", "2030-01-06"].includes(date) ? "Closed" : "NoTrades", bars: [], daily_candle: null })), next_cursor: null, settled_through: "2030-01-09" };
  }, "600001", "2030-01-10");
  assert.deepEqual(page.entries.map((entry) => entry.date), ["2030-01-03", "2030-01-04", "2030-01-07", "2030-01-08", "2030-01-09"]);
  assert.equal(requests.length, 1);
  assert.ok(page.entries.every((entry) => entry.bars.length === 0));
});

test("开局不足五日明确返回真实已结束日，并遵守分页游标", { timeout: 10000 }, async () => {
  const requests: { after: string | null }[] = [];
  const page = await latestFiveTradingDays(async (request) => {
    requests.push(request);
    const dates = request.after === null ? ["2029-12-30", "2029-12-31", "2030-01-01"] : ["2030-01-02", "2030-01-03", "2030-01-04", "2030-01-05"];
    return { code: request.code, entries: dates.map((date) => ({ date, availability: date < "2030-01-02" ? "BeforeStart" : date === "2030-01-05" ? "Closed" : "NoTrades", bars: [], daily_candle: null })), next_cursor: request.after === null ? "2030-01-01" : null, settled_through: "2030-01-05" };
  }, "600001", "2030-01-06");
  assert.deepEqual(page.entries.map((entry) => entry.date), ["2030-01-02", "2030-01-03", "2030-01-04"]);
  assert.deepEqual(requests.map((request) => request.after), [null, "2030-01-01"]);
});

test("读取失败显式抛出，不把错误当不足五日", { timeout: 10000 }, async () => {
  await assert.rejects(latestFiveTradingDays(async () => { throw new Error("历史不可用"); }, "600001", "2030-01-10"), /历史不可用/);
});
