import assert from "node:assert/strict";
import test from "node:test";
import { parseDailyCandle as parseWireCandle } from "./protocol/wire-values.ts";
import { parseDailyCandle as parseSavedCandle } from "../save/schema/market.ts";
import { parseTurnoverCents, U128_MAX } from "../utils/turnover.ts";
import { normalizeIntradayAverageCurveInput, normalizeIntradayAverageCurveResult, normalizePersonalTradeConfirmations } from "./intraday-average.ts";

test("累计成交额跨日K存档协议及VWAP保留完整u128字符串", { timeout: 10000 }, () => {
  const turnover = "20000000000000000000";
  const candle = { time: 1893456000, open: "5000000000000000000", high: "5000000000000000000", low: "5000000000000000000", close: "5000000000000000000", volume: 4, trade_stats: { turnover_cents: turnover, trade_count: 4 } };
  assert.equal(parseWireCandle(candle, "candle").trade_stats?.turnover_cents, turnover);
  assert.equal(parseSavedCandle(candle, "candle").trade_stats?.turnover_cents, turnover);
  assert.deepEqual(normalizeIntradayAverageCurveInput({ seriesKey: "wide", samples: [{ turnoverCents: turnover, tradeCount: 4, volumeShares: 4 }] }).samples[0]?.turnoverCents, turnover);
  assert.deepEqual(normalizeIntradayAverageCurveResult([{ turnover_cents: turnover, volume_shares: 4 }], 1), [{ turnoverCents: turnover, volumeShares: 4 }]);
});

test("累计成交额只接受规范u128字符串而非单账户Money或数值", { timeout: 10000 }, () => {
  assert.equal(parseTurnoverCents(U128_MAX.toString(), "turnover"), U128_MAX.toString());
  for (const invalid of [1, "", "00", "01", "+1", "-1", " 1", "1.0", (U128_MAX + 1n).toString()]) {
    assert.throws(() => parseTurnoverCents(invalid, "turnover"));
    const candle = { time: 1893456000, open: "1", high: "1", low: "1", close: "1", volume: 1, trade_stats: { turnover_cents: invalid, trade_count: 1 } };
    assert.throws(() => parseWireCandle(candle, "candle"));
    assert.throws(() => parseSavedCandle(candle, "candle"));
    assert.throws(() => normalizeIntradayAverageCurveResult([{ turnover_cents: invalid, volume_shares: 1 }], 1));
  }
});

test("本人交割单严格保留完整u64 receipt与i64分金额边界", { timeout: 10000 }, () => {
  const row = { receipt_id: "18446744073709551615", civil_date: "2030-01-02", code: "600000", side: "Buy", price: "1000", quantity_shares: 100, gross: "100000", actual_fees: { commission: "500", stamp_tax: "0", transfer_fee: "1" } };
  assert.equal(normalizePersonalTradeConfirmations([row])[0]?.receipt_id, row.receipt_id);
  assert.throws(() => normalizePersonalTradeConfirmations([{ ...row, receipt_id: "18446744073709551616" }]), /u64/);
  assert.throws(() => normalizePersonalTradeConfirmations([{ ...row, actual_fees: { ...row.actual_fees, commission: "9223372036854775808" } }]), /i64/);
  assert.throws(() => normalizePersonalTradeConfirmations([{ ...row, quantity_shares: 4294967296 }]), /u32/);
});

test("VWAP curve 校验同一请求键与有界样本并保留权威成交统计", { timeout: 10000 }, () => {
  assert.deepEqual(normalizeIntradayAverageCurveInput({
    seriesKey: "600000:1:901",
    samples: [{ turnoverCents: "260000", tradeCount: 2, volumeShares: 300 }],
  }), {
    seriesKey: "600000:1:901",
    samples: [{ turnoverCents: "260000", tradeCount: 2, volumeShares: 300 }],
  });
  assert.throws(() => normalizeIntradayAverageCurveInput({ seriesKey: "", samples: [] }), /seriesKey/);
  assert.throws(() => normalizeIntradayAverageCurveInput({ seriesKey: "600000", samples: [{ turnoverCents: "026", tradeCount: 1, volumeShares: 1 }] }), /规范/);
});

test("VWAP curve 拒绝错位或长度不匹配的宿主结果", { timeout: 10000 }, () => {
  assert.deepEqual(normalizeIntradayAverageCurveResult([
    { turnover_cents: "260000", volume_shares: 300 }, null,
  ], 2), [{ turnoverCents: "260000", volumeShares: 300 }, null]);
  assert.throws(() => normalizeIntradayAverageCurveResult([{ turnover_cents: "260000", volume_shares: 300 }], 2), /长度/);
  assert.throws(() => normalizeIntradayAverageCurveResult([{ turnover_cents: "1", volume_shares: 0 }], 1), /正安全整数/);
});

test("Rust fulfilled回包统计不符原始facts时显式拒绝，不能安装错位均价", { timeout: 10000 }, () => {
  const samples = [{ turnoverCents: "260000", tradeCount: 2, volumeShares: 300 }];
  assert.throws(() => normalizeIntradayAverageCurveResult([{ turnover_cents: "100000", volume_shares: 100 }], 1, samples), /事实/);
  assert.throws(() => normalizeIntradayAverageCurveResult([null], 1, samples), /事实/);
});
