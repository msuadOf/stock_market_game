import assert from "node:assert/strict";
import test from "node:test";
import { parseDailyCandle as parseWireCandle } from "./protocol/wire-values.ts";
import { parseDailyCandle as parseSavedCandle } from "../save/schema/market.ts";
import { parseTurnoverCents, U128_MAX } from "../utils/turnover.ts";
import { normalizeIntradayAverageCurveInput, normalizeIntradayAverageCurveResult, normalizePersonalTradeConfirmations } from "./intraday-average.ts";

test("VWAP wire 保留完整 u128 字符串（日K stats 为 B2 u64 域）", { timeout: 10000 }, () => {
  const turnover = "20000000000000000000";
  // B2 契约：DailyTradeStats.turnover_cents 为 u64（会话域）；VWAP 输入/结果
  // 沿用 u128 wire（合法成交额可超 u64——分钟域 MinuteBarFact 同口径）。
  const candle = { time: 1893456000, open: "5000000000000000000", high: "5000000000000000000", low: "5000000000000000000", close: "5000000000000000000", volume: 4, trade_stats: { turnover_cents: "18446744073709551615", trade_count: 4 } };
  assert.equal(parseWireCandle(candle, "candle").trade_stats?.turnover_cents, "18446744073709551615");
  assert.equal(parseSavedCandle(candle, "candle").trade_stats?.turnover_cents, "18446744073709551615");
  assert.throws(() => parseWireCandle({ ...candle, trade_stats: { turnover_cents: turnover, trade_count: 4 } }, "candle"), /u64/);
  assert.deepEqual(normalizeIntradayAverageCurveInput({ seriesKey: "wide", samples: [{ turnoverCents: turnover, tradeCount: 4, volumeShares: 4 }] }).samples[0]?.turnoverCents, turnover);
  assert.deepEqual(normalizeIntradayAverageCurveResult([{ turnover_cents: turnover, volume_shares: 4 }], 1), [{ turnoverCents: turnover, volumeShares: 4 }]);
});

test("VWAP 累计成交额只接受规范u128字符串而非单账户Money或数值", { timeout: 10000 }, () => {
  assert.equal(parseTurnoverCents(U128_MAX.toString(), "turnover"), U128_MAX.toString());
  for (const invalid of [1, "", "00", "01", "+1", "-1", " 1", "1.0", (U128_MAX + 1n).toString()]) {
    assert.throws(() => parseTurnoverCents(invalid, "turnover"));
    assert.throws(() => normalizeIntradayAverageCurveResult([{ turnover_cents: invalid, volume_shares: 1 }], 1));
  }
  for (const invalid of [1, "", "+1", "-1", "1.0", "18446744073709551616"]) {
    // 日K stats 域（u64）：wire-values 按无符号 u64 纪律拒绝（存档侧
    // parseSavedCandle 走宽松 decimal——两解析器既有差异，不在此断言；
    // "00" 前导零为 wire-values 既有宽松面，不在本批收紧范围）。
    const candle = { time: 1893456000, open: "1", high: "1", low: "1", close: "1", volume: 1, trade_stats: { turnover_cents: invalid, trade_count: 1 } };
    assert.throws(() => parseWireCandle(candle, "candle"));
  }
  for (const invalid of [1, "", "+1", "-1", "1.0"]) {
    const candle = { time: 1893456000, open: "1", high: "1", low: "1", close: "1", volume: 1, trade_stats: { turnover_cents: invalid, trade_count: 1 } };
    assert.throws(() => parseSavedCandle(candle, "candle"));
  }
});

test("本人交割单严格保留完整u64 receipt与i64分金额边界（B2 wire：stock/price_cents/gross_cents）", { timeout: 10000 }, () => {
  // 000812 ↔ StockId 0（STOCK_LIST 代码升序，ADR 0041）。
  const row = { receipt_id: "18446744073709551615", civil_date: "2030-01-02", stock: 0, side: "Buy", price_cents: "1000", quantity_shares: 100, gross_cents: "100000", actual_fees: { commission: "500", stamp_tax: "0", transfer_fee: "1" } };
  const normalized = normalizePersonalTradeConfirmations([row])[0]!;
  assert.equal(normalized.receipt_id, row.receipt_id);
  assert.equal(normalized.code, "000812");
  assert.equal(normalized.price, "1000");
  assert.equal(normalized.gross, "100000");
  assert.throws(() => normalizePersonalTradeConfirmations([{ ...row, receipt_id: "18446744073709551616" }]), /u64/);
  assert.throws(() => normalizePersonalTradeConfirmations([{ ...row, actual_fees: { ...row.actual_fees, commission: "9223372036854775808" } }]), /i64/);
  assert.throws(() => normalizePersonalTradeConfirmations([{ ...row, quantity_shares: 4294967296 }]), /u32/);
  assert.throws(() => normalizePersonalTradeConfirmations([{ ...row, stock: 99 }]), /未知股票/);
  assert.throws(() => normalizePersonalTradeConfirmations([{ ...row, price_cents: "999" }]), /成交价、成交量与成交额不一致/);
});

test("VWAP curve 校验同一请求键与有界样本并保留权威成交统计", { timeout: 10000 }, () => {
  assert.deepEqual(normalizeIntradayAverageCurveInput({
    seriesKey: "000812:1:901",
    samples: [{ turnoverCents: "260000", tradeCount: 2, volumeShares: 300 }],
  }), {
    seriesKey: "000812:1:901",
    samples: [{ turnoverCents: "260000", tradeCount: 2, volumeShares: 300 }],
  });
  assert.throws(() => normalizeIntradayAverageCurveInput({ seriesKey: "", samples: [] }), /seriesKey/);
  assert.throws(() => normalizeIntradayAverageCurveInput({ seriesKey: "000812", samples: [{ turnoverCents: "026", tradeCount: 1, volumeShares: 1 }] }), /规范/);
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
