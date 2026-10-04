import assert from "node:assert/strict";
import test from "node:test";
import { candleDate } from "./candle-date.ts";
import { parseDailyCandle as parseHostCandle } from "../host/protocol/wire-values.ts";
import { parseDailyCandle as parseSaveCandle } from "../save/schema/market.ts";
import { ProtocolError } from "../host/protocol/types.ts";

test("两层边界采用同一明确公历日期校验且拒绝旧相对日期", { timeout: 10000 }, () => {
  const time = Date.parse("2030-01-02T00:00:00Z") / 1000;
  const candle = { time, open: "1000", high: "1000", low: "1000", close: "1000", volume: 0 };
  assert.equal(candleDate(time).toISOString(), "2030-01-02T00:00:00.000Z");
  assert.deepEqual(parseHostCandle(candle, "host.daily"), candle);
  assert.deepEqual(parseSaveCandle(candle, "save.daily"), candle);
  for (const invalidTime of [0, -86400, time + 1, Date.parse("2100-01-01T00:00:00Z") / 1000]) {
    assert.throws(() => parseHostCandle({ ...candle, time: invalidTime }, "host.daily"), (error) => error instanceof ProtocolError && error.where === "host.daily.time");
    assert.throws(() => parseSaveCandle({ ...candle, time: invalidTime }, "save.daily"), /save\.daily\.time/);
  }
  assert.equal(candleDate(Date.parse("1998-01-01T00:00:00Z") / 1000).getUTCFullYear(), 1998);
  assert.equal(candleDate(Date.parse("2099-12-31T00:00:00Z") / 1000).getUTCFullYear(), 2099);
});
