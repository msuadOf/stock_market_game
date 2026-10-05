import { parseTurnoverCents } from "../../utils/turnover.ts"
import type { DailyCandle } from "../../types/generated/DailyCandle.ts"
import { SaveSchemaError, array, civilDate, decimal, exact, integer, money, oneOf, record } from "./primitives.ts"

type Stock = { readonly code: string; readonly tick: string }
type Configuration = { readonly auction_ticks: number; readonly closing_auction_ticks: number }
export type SavedMinuteBar = { minute_of_day: number; phase: "OpenAuction" | "Continuous" | "ClosingAuction"; open: string; high: string; low: string; close: string; volume_shares: string; turnover_cents: string; trade_count: string }

function counter(value: unknown, path: string): string {
  const result = decimal(value, path)
  if (!/^(0|[1-9]\d*)$/.test(result)) throw new SaveSchemaError(path, "必须为规范 u64 十进制字符串")
  return result
}

export function parseMinuteBars(value: unknown, path: string, stock?: Stock, config?: Configuration): SavedMinuteBar[] {
  let previous = -1
  return array(value, path).map((item, index) => {
    const location = `${path}[${index}]`
    const parsed = record(item, location)
    exact(parsed, ["minute_of_day", "phase", "open", "high", "low", "close", "volume_shares", "turnover_cents", "trade_count"], location)
    const result: SavedMinuteBar = {
      minute_of_day: integer(parsed.minute_of_day, `${location}.minute_of_day`, 0),
      phase: oneOf(parsed.phase, `${location}.phase`, ["OpenAuction", "Continuous", "ClosingAuction"] as const),
      open: money(parsed.open, `${location}.open`), high: money(parsed.high, `${location}.high`), low: money(parsed.low, `${location}.low`), close: money(parsed.close, `${location}.close`),
      volume_shares: counter(parsed.volume_shares, `${location}.volume_shares`), trade_count: counter(parsed.trade_count, `${location}.trade_count`), turnover_cents: parseTurnoverCents(parsed.turnover_cents, `${location}.turnover_cents`),
    }
    const minute = result.minute_of_day
    const validTime = result.phase === "OpenAuction" ? minute === 565 : result.phase === "ClosingAuction" ? minute === 900 : (minute >= 570 && minute < 690) || (minute >= 780 && minute < 900)
    if (!validTime || minute <= previous) throw new SaveSchemaError(location, "交易时段不合法或分钟乱序、重复")
    previous = minute
    const low = BigInt(result.low), high = BigInt(result.high), volume = BigInt(result.volume_shares), count = BigInt(result.trade_count), turnover = BigInt(result.turnover_cents)
    if (low <= 0n || high < low || BigInt(result.open) < low || BigInt(result.open) > high || BigInt(result.close) < low || BigInt(result.close) > high || volume === 0n || count === 0n || count > volume || turnover < low * volume || turnover > high * volume) throw new SaveSchemaError(location, "OHLC、股数、笔数或成交额约束不合法")
    if (stock !== undefined && (BigInt(stock.tick) <= 0n || [result.open, result.high, result.low, result.close].some((price) => BigInt(price) % BigInt(stock.tick) !== 0n))) throw new SaveSchemaError(location, "价格不符合证券最小变动单位")
    if (config !== undefined && ((result.phase === "OpenAuction" && config.auction_ticks === 0) || (result.phase === "ClosingAuction" && config.closing_auction_ticks === 0) || (result.phase === "Continuous" && config.closing_auction_ticks > 0 && minute >= 897))) throw new SaveSchemaError(location, "阶段或时刻不符合本局竞价配置")
    return result
  })
}

export function parseActiveMinuteHistory(value: unknown, path = "runtime_state.active_minute_history"): Record<string, SavedMinuteBar[]> {
  const parsed = record(value, path)
  const result: Record<string, SavedMinuteBar[]> = {}
  for (const [code, bars] of Object.entries(parsed)) {
    if (code.length === 0) throw new SaveSchemaError(path, "证券代码不能为空")
    result[code] = parseMinuteBars(bars, `${path}.${code}`)
    if (result[code].length === 0) throw new SaveSchemaError(`${path}.${code}`, "活动分钟不能包含空数组")
  }
  return result
}

type HistoryContext = Configuration & { readonly stocks: readonly Stock[]; readonly start_date: string; readonly settled_through: string | null }
function nextDate(date: string): string {
  const instant = new Date(`${date}T00:00:00Z`)
  instant.setUTCDate(instant.getUTCDate() + 1)
  return instant.toISOString().slice(0, 10)
}

export function parseRetainedHistory(value: unknown, context: HistoryContext) {
  const path = "retained_market_history"
  const values = array(value, path)
  let expected = context.start_date
  const result = values.map((item, index) => {
    const location = `${path}[${index}]`
    const parsed = record(item, location)
    exact(parsed, ["date", "securities"], location)
    const date = civilDate(parsed.date, `${location}.date`)
    if (date !== expected || context.settled_through === null || date > context.settled_through) throw new SaveSchemaError(`${location}.date`, "日期必须连续覆盖已结束自然日")
    expected = nextDate(date)
    const securities = record(parsed.securities, `${location}.securities`)
    exact(securities, context.stocks.map((stock) => stock.code), `${location}.securities`)
    const sessions: Record<string, { status: "Trading" | "Closed"; bars: SavedMinuteBar[] }> = {}
    for (const stock of context.stocks) {
      const sessionPath = `${location}.securities.${stock.code}`
      const session = record(securities[stock.code], sessionPath)
      exact(session, ["status", "bars"], sessionPath)
      const status = oneOf(session.status, `${sessionPath}.status`, ["Trading", "Closed"] as const)
      const bars = parseMinuteBars(session.bars, `${sessionPath}.bars`, stock, context)
      if (status === "Closed" && bars.length > 0) throw new SaveSchemaError(sessionPath, "休市证券不能存在成交分钟")
      sessions[stock.code] = { status, bars }
    }
    return { date, securities: sessions }
  })
  if ((context.settled_through === null && result.length !== 0) || (context.settled_through !== null && expected !== nextDate(context.settled_through))) throw new SaveSchemaError(path, "历史未完整覆盖所有已结束自然日")
  return result
}

export function validateRetainedHistoryCandles(days: ReturnType<typeof parseRetainedHistory>, candles: Readonly<Record<string, readonly DailyCandle[]>>): void {
  for (const day of days) for (const [code, session] of Object.entries(day.securities)) {
    const path = `retained_market_history.${day.date}.${code}`
    const time = Date.parse(`${day.date}T00:00:00Z`) / 1000
    const candle = candles[code]?.find((item) => item.time === time)
    if (session.status === "Closed") {
      if (candle !== undefined) throw new SaveSchemaError(path, "休市证券不能存在当日日K")
      continue
    }
    if (candle === undefined || candle.trade_stats === undefined || candle.trade_stats === null) throw new SaveSchemaError(path, "开市历史缺少真实日K成交统计")
    const totals = session.bars.reduce((sum, bar) => ({ volume: sum.volume + BigInt(bar.volume_shares), turnover: sum.turnover + BigInt(bar.turnover_cents), count: sum.count + BigInt(bar.trade_count) }), { volume: 0n, turnover: 0n, count: 0n })
    if (totals.volume !== BigInt(candle.volume) || totals.turnover !== BigInt(candle.trade_stats.turnover_cents) || totals.count !== BigInt(candle.trade_stats.trade_count)) throw new SaveSchemaError(path, "分钟累计量价与真实日K不一致")
    const first = session.bars[0], last = session.bars.at(-1)
    if (first !== undefined && last !== undefined && (first.open !== candle.open || last.close !== candle.close || session.bars.some((bar) => BigInt(bar.high) > BigInt(candle.high) || BigInt(bar.low) < BigInt(candle.low)) || !session.bars.some((bar) => bar.high === candle.high) || !session.bars.some((bar) => bar.low === candle.low))) throw new SaveSchemaError(path, "分钟OHLC与真实日K不一致")
  }
}
