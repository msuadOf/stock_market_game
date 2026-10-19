/**
 * B2 保存域历史字段的严格 schema（W3 取回自 18725d07，按 B2 新存档 wire 适配）。
 *
 * 旧 main 的四个域字段（runtime_state.active_minute_history /
 * retained_market_history / runtime_state.personal_trade_confirmations）在
 * B2 重建后为存档根级四字段（ADR-0041 数值键词表）：
 * - `market_minute_bars`：当日分钟K桶（StockId 数值键；零成交分钟不造 bar）；
 * - `retained_market_days`：日结冻结的不可变日快照链（`SharedRetainedDay`）；
 * - `personal_trade_confirmations`：按账户平铺的 receipt 日志（wire 行）；
 * - `receipt_watermark`：回执水位（u64 安全整数）。
 *
 * 校验语义对照旧基线逐条保留：分钟 OHLCV/量额笔互检、证券最小变动单位、
 * 竞价配置相位时刻、保留链日期连续覆盖 settled_through、休市日零 bar、
 * 分钟累计与真实日K统计逐位一致、receipt 行的证券/日期归属。
 */
import { parseTurnoverCents } from "../../utils/turnover.ts"
import type { DailyCandle } from "../../types/generated/DailyCandle.ts"
import { SaveSchemaError, array, civilDate, decimal, exact, integer, money, oneOf, record } from "./primitives.ts"

/** 逐条证券约束（StockId → 最小变动单位；由根层从 setup 推导）。 */
export type StockTickById = { readonly id: string; readonly tick: string }[]
type Configuration = { readonly auction_ticks: number; readonly closing_auction_ticks: number }

export type SavedMinuteBar = { minute_of_day: number; phase: "OpenAuction" | "Continuous" | "ClosingAuction"; open: string; high: string; low: string; close: string; volume_shares: string; turnover_cents: string; trade_count: string }

function counter(value: unknown, path: string): string {
  const result = decimal(value, path)
  if (!/^(0|[1-9]\d*)$/.test(result)) throw new SaveSchemaError(path, "必须为规范 u64 十进制字符串")
  return result
}

export function parseMinuteBars(value: unknown, path: string, stock?: { readonly tick: string }, config?: Configuration): SavedMinuteBar[] {
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

/** 当日分钟K桶（根级 `market_minute_bars`，StockId 数值键——零成交股可为空数组）。 */
export function parseActiveMinuteBars(value: unknown, path = "market_minute_bars", stocks?: StockTickById, config?: Configuration): Record<string, SavedMinuteBar[]> {
  const parsed = record(value, path)
  const result: Record<string, SavedMinuteBar[]> = {}
  for (const [id, bars] of Object.entries(parsed)) {
    if (!/^\d+$/.test(id)) throw new SaveSchemaError(`${path}.${id}`, "键必须为数值 StockId 的字符串形态")
    const stock = stocks?.find((stock) => stock.id === id)
    if (stocks !== undefined && stock === undefined) throw new SaveSchemaError(`${path}.${id}`, "证券不存在（StockId 超出 setup 目录）")
    result[id] = parseMinuteBars(bars, `${path}.${id}`, stock, config)
  }
  return result
}

export type SavedRetainedDay = { readonly date: string; readonly securities: Readonly<Record<string, { readonly trading: boolean; readonly bars: readonly SavedMinuteBar[] }>> }

type HistoryContext = Configuration & { readonly stocks: StockTickById; readonly start_date: string; readonly settled_through: string | null }

function nextDate(date: string): string {
  const instant = new Date(`${date}T00:00:00Z`)
  instant.setUTCDate(instant.getUTCDate() + 1)
  return instant.toISOString().slice(0, 10)
}

/** 保留链（根级 `retained_market_days`）：日期连续覆盖已结束自然日、
 *  每证券必有条目（开市/休市都留）、休市（trading=false）零 bar。 */
export function parseRetainedHistory(value: unknown, context: HistoryContext): SavedRetainedDay[] {
  const path = "retained_market_days"
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
    exact(securities, context.stocks.map((stock) => stock.id), `${location}.securities`)
    const sessions: Record<string, { trading: boolean; bars: readonly SavedMinuteBar[] }> = {}
    for (const stock of context.stocks) {
      const sessionPath = `${location}.securities.${stock.id}`
      const session = record(securities[stock.id], sessionPath)
      exact(session, ["trading", "bars"], sessionPath)
      const trading = session.trading
      if (typeof trading !== "boolean") throw new SaveSchemaError(sessionPath, "trading 必须为布尔值（旧 status 二值词表）")
      const bars = parseMinuteBars(session.bars, `${sessionPath}.bars`, stock, context)
      if (!trading && bars.length > 0) throw new SaveSchemaError(sessionPath, "休市证券不能存在成交分钟")
      sessions[stock.id] = { trading, bars }
    }
    return { date, securities: sessions }
  })
  if ((context.settled_through === null && result.length !== 0) || (context.settled_through !== null && expected !== nextDate(context.settled_through))) throw new SaveSchemaError(path, "历史未完整覆盖所有已结束自然日")
  return result
}

/** 分钟累计量价与真实日K统计逐位一致（开市日必有带 trade_stats 的日K）。 */
export function validateRetainedHistoryCandles(days: readonly SavedRetainedDay[], candles: Readonly<Record<string, readonly DailyCandle[]>>): void {
  for (const day of days) for (const [id, session] of Object.entries(day.securities)) {
    const path = `retained_market_days.${day.date}.${id}`
    const time = Date.parse(`${day.date}T00:00:00Z`) / 1000
    const candle = candles[id]?.find((item) => item.time === time)
    if (!session.trading) {
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

export type SavedTradeReceipt = { readonly receipt_id: string; readonly civil_date: string; readonly stock: string; readonly side: "Buy" | "Sell"; readonly price_cents: string; readonly quantity_shares: number; readonly gross_cents: string; readonly actual_fees: { readonly commission: string; readonly stamp_tax: string; readonly transfer_fee: string } }

/** receipt 日志（根级 `personal_trade_confirmations`，AccountId 数值键——
 *  B2 wire 行：stock 数值 StockId + price_cents/gross_cents 分串）。 */
export function parsePersonalTradeConfirmations(value: unknown, path = "personal_trade_confirmations"): Record<string, SavedTradeReceipt[]> {
  const parsed = record(value, path)
  const result: Record<string, SavedTradeReceipt[]> = {}
  for (const [account, rows] of Object.entries(parsed)) {
    const accountPath = `${path}.${account}`
    if (!/^\d+$/.test(account)) throw new SaveSchemaError(accountPath, "键必须为数值 AccountId 的字符串形态")
    result[account] = array(rows, accountPath).map((row, index) => {
      const location = `${accountPath}[${index}]`
      const item = record(row, location)
      exact(item, ["receipt_id", "civil_date", "stock", "side", "price_cents", "quantity_shares", "gross_cents", "actual_fees"], location)
      const fees = record(item.actual_fees, `${location}.actual_fees`)
      exact(fees, ["commission", "stamp_tax", "transfer_fee"], `${location}.actual_fees`)
      const quantity = integer(item.quantity_shares, `${location}.quantity_shares`, 1)
      const price = money(item.price_cents, `${location}.price_cents`)
      const gross = money(item.gross_cents, `${location}.gross_cents`)
      if (typeof item.stock !== "string" || !/^\d+$/.test(item.stock)) throw new SaveSchemaError(`${location}.stock`, "必须为数值 StockId 的字符串形态")
      if (price === "0" || BigInt(price) * BigInt(quantity) !== BigInt(gross)) throw new SaveSchemaError(location, "成交价、成交量与成交额不一致")
      return {
        receipt_id: counter(item.receipt_id, `${location}.receipt_id`),
        civil_date: civilDate(item.civil_date, `${location}.civil_date`),
        stock: item.stock,
        side: oneOf(item.side, `${location}.side`, ["Buy", "Sell"] as const),
        price_cents: price,
        quantity_shares: quantity,
        gross_cents: gross,
        actual_fees: { commission: money(fees.commission, `${location}.actual_fees.commission`), stamp_tax: money(fees.stamp_tax, `${location}.actual_fees.stamp_tax`), transfer_fee: money(fees.transfer_fee, `${location}.actual_fees.transfer_fee`) },
      }
    })
    // 单调递增（AppendOnlyHistory 尾追加；同 id 重复=双投影结构性 bug）。
    for (let index = 1; index < result[account]!.length; index += 1) {
      if (BigInt(result[account][index - 1]!.receipt_id) >= BigInt(result[account][index]!.receipt_id)) throw new SaveSchemaError(`${accountPath}[${index}]`, "receipt_id 必须严格递增")
    }
  }
  return result
}

/** 回执水位（根级 `receipt_watermark`）——有已发 receipt 时必须超过最大 id
 *  （水位=下一待发号）；空日志时水位 0 为规范初值（next_receipt_base）。 */
export function parseReceiptWatermark(value: unknown, receipts: Readonly<Record<string, readonly SavedTradeReceipt[]>>): number {
  const watermark = integer(value, "receipt_watermark", 0)
  const rows = Object.values(receipts).flat()
  if (rows.length === 0) return watermark
  const maximum = rows.reduce((max, row) => BigInt(row.receipt_id) > max ? BigInt(row.receipt_id) : max, 0n)
  if (BigInt(watermark) <= maximum) throw new SaveSchemaError("receipt_watermark", "水位必须超过最大已发 receipt_id")
  return watermark
}
