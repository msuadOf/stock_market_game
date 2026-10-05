import assert from "node:assert/strict"
import test from "node:test"
import { parseMinuteBars, parseRetainedHistory, parseActiveMinuteHistory, validateRetainedHistoryCandles } from "./retained-history.ts"

const bar = { minute_of_day: 570, phase: "Continuous", open: "1000", high: "1000", low: "1000", close: "1000", volume_shares: "100", turnover_cents: "100000", trade_count: "1" }
const stock = { code: "600000", tick: "1" }
const config = { auction_ticks: 0, closing_auction_ticks: 0 }

test("分钟事实严格保留规范金额、股数、时段与价格单位", { timeout: 10000 }, () => {
  assert.deepEqual(parseMinuteBars([bar], "bars", stock, config), [bar])
  for (const patch of [{ volume_shares: "01" }, { trade_count: 1 }, { turnover_cents: "340282366920938463463374607431768211456" }, { minute_of_day: 690 }, { phase: "OpenAuction", minute_of_day: 565 }, { low: "1001" }, { turnover_cents: "99999" }, { maker: "1" }]) assert.throws(() => parseMinuteBars([{ ...bar, ...patch }], "bars", stock, config))
  assert.throws(() => parseMinuteBars([bar, bar], "bars", stock, config), /乱序|重复/)
  assert.throws(() => parseMinuteBars([{ ...bar, open: "1001", high: "1001", close: "1001", turnover_cents: "100100" }], "bars", { ...stock, tick: "5" }, config), /变动单位/)
  assert.throws(() => parseMinuteBars([{ ...bar, minute_of_day: 898 }], "bars", stock, { ...config, closing_auction_ticks: 1 }), /竞价配置/)
})

test("完整日终历史覆盖每个自然日和精确证券集合", { timeout: 10000 }, () => {
  const days = [{ date: "2030-01-02", securities: { "600000": { status: "Trading", bars: [bar] } } }, { date: "2030-01-03", securities: { "600000": { status: "Closed", bars: [] } } }]
  const context = { stocks: [stock], ...config, start_date: "2030-01-02", settled_through: "2030-01-03" }
  assert.deepEqual(parseRetainedHistory(days, context), days)
  for (const changed of [undefined, [], days.slice(1), [...days, days[1]], [{ ...days[0], securities: {} }, days[1]], [{ ...days[0], date: "2030-01-01" }, days[1]], [days[0], { ...days[1], securities: { "600000": { status: "Closed", bars: [bar] } } }]]) assert.throws(() => parseRetainedHistory(changed, context))
  assert.deepEqual(parseRetainedHistory([], { ...context, settled_through: null }), [])
})

test("活动分钟字段必填且仅接受非空的真实稀疏事实", { timeout: 10000 }, () => {
  assert.deepEqual(parseActiveMinuteHistory({}), {})
  assert.deepEqual(parseActiveMinuteHistory({ "600000": [bar] }), { "600000": [bar] })
  assert.throws(() => parseActiveMinuteHistory(undefined))
  assert.throws(() => parseActiveMinuteHistory({ "600000": [] }), /空数组/)
})

test("归档分钟和日K逐项互证而非补造成交", { timeout: 10000 }, () => {
  const days = parseRetainedHistory([{ date: "2030-01-02", securities: { "600000": { status: "Trading", bars: [bar] } } }], { stocks: [stock], ...config, start_date: "2030-01-02", settled_through: "2030-01-02" })
  const candle = { time: Date.parse("2030-01-02T00:00:00Z") / 1000, open: "1000", high: "1000", low: "1000", close: "1000", volume: 100, trade_stats: { turnover_cents: "100000", trade_count: 1 } }
  validateRetainedHistoryCandles(days, { "600000": [candle] })
  assert.throws(() => validateRetainedHistoryCandles(days, {}), /缺少/)
  assert.throws(() => validateRetainedHistoryCandles(days, { "600000": [{ ...candle, volume: 101 }] }), /累计量价/)
  assert.throws(() => validateRetainedHistoryCandles(days, { "600000": [{ ...candle, high: "1001" }] }), /OHLC/)
  const closed = [{ date: "2030-01-02", securities: { "600000": { status: "Closed" as const, bars: [] } } }]
  assert.throws(() => validateRetainedHistoryCandles(closed, { "600000": [candle] }), /休市/)
})

test("零成交开市与闰日连续覆盖不裁剪且不生成分钟", { timeout: 10000 }, () => {
  const dates = ["2032-02-28", "2032-02-29", "2032-03-01"]
  const days = parseRetainedHistory(dates.map((date) => ({ date, securities: { "600000": { status: "Trading", bars: [] } } })), { stocks: [stock], ...config, start_date: dates[0]!, settled_through: dates[2]! })
  assert.deepEqual(days.map((day) => day.date), dates)
  const candles = dates.map((date) => ({ time: Date.parse(`${date}T00:00:00Z`) / 1000, open: "1000", high: "1000", low: "1000", close: "1000", volume: 0, trade_stats: { turnover_cents: "0", trade_count: 0 } }))
  validateRetainedHistoryCandles(days, { "600000": candles })
  assert.ok(days.every((day) => day.securities["600000"]!.bars.length === 0))
})
