import { money as parseCanonicalMoney } from "./primitives.ts"
import { candleDate } from "../../utils/candle-date.ts"
import { parseTurnoverCents } from "../../utils/turnover.ts"
import type { FloatAllocation, SessionSetup, Snapshot } from "../../types/engine"
import type { DailyCandle } from "../../types/generated/DailyCandle"
import type { ExReferencePrice } from "./company/ex-reference-price.ts"
import { parseCompanySystemConfig } from "./company/system-config.ts"
import { parseReportFrequency } from "./report-frequency.ts"
import { array, boolean, civilDate, decimal, exact, finite, integer, map, nullable, oneOf, record, accountKey, string, SaveSchemaError } from "./primitives.ts"

const exchange = ["Shanghai", "Shenzhen"] as const
const category = ["MainBoard", "StMainBoard", "ChiNext"] as const
const phase = ["CallAuction", "PreOpen", "ClosingAuction", "Continuous"] as const

function stockCode(value: string, path: string): void {
  if (value.length === 0) throw new Error(`存档 ${path} 股票代码不能为空`)
}

function money(value: unknown, path: string): string {
  return parseCanonicalMoney(value, path)
}

function stock(value: unknown, path: string): SessionSetup["stocks"][number] {
  const parsed = record(value, path)
  exact(parsed, ["code", "exchange", "initial_price", "category", "limit_pct", "tick", "total_shares", "float_shares"], path)
  const code = string(parsed.code, `${path}.code`)
  stockCode(code, `${path}.code`)
  return {
    code,
    exchange: oneOf(parsed.exchange, `${path}.exchange`, exchange),
    initial_price: money(parsed.initial_price, `${path}.initial_price`),
    category: oneOf(parsed.category, `${path}.category`, category),
    limit_pct: finite(parsed.limit_pct, `${path}.limit_pct`),
    tick: money(parsed.tick, `${path}.tick`),
    total_shares: decimal(parsed.total_shares, `${path}.total_shares`),
    float_shares: integer(parsed.float_shares, `${path}.float_shares`, 0),
  }
}

function floatAllocation(value: unknown, path: string): FloatAllocation {
  const parsed = record(value, path)
  exact(parsed, ["between_kinds", "within_kind"], path)
  let betweenKinds: FloatAllocation["between_kinds"]
  if (parsed.between_kinds === "Random") {
    betweenKinds = parsed.between_kinds
  } else {
    const percentage = record(parsed.between_kinds, `${path}.between_kinds`)
    exact(percentage, ["Percentage"], `${path}.between_kinds`)
    const weights = record(percentage.Percentage, `${path}.between_kinds.Percentage`)
    exact(weights, ["retail", "inst", "hot"], `${path}.between_kinds.Percentage`)
    betweenKinds = { Percentage: {
      retail: finite(weights.retail, `${path}.between_kinds.Percentage.retail`),
      inst: finite(weights.inst, `${path}.between_kinds.Percentage.inst`),
      hot: finite(weights.hot, `${path}.between_kinds.Percentage.hot`),
    } }
  }
  const withinKind = oneOf(parsed.within_kind, `${path}.within_kind`, ["Random", "EqualPercentage"] as const)
  return { between_kinds: betweenKinds, within_kind: withinKind }
}

export function parseSetup(value: unknown, path: string): SessionSetup {
  const parsed = record(value, path)
  exact(parsed, ["stocks", "npcs", "config", "strategy_params", "ticks_per_day", "auction_ticks", "closing_auction_ticks", "history_len", "t1_enabled", "float_allocation", "report_frequency", "company_system", "start_date", "simulation_policy_id", "dividend_tax_mode"], path)
  const npcs = record(parsed.npcs, `${path}.npcs`)
  exact(npcs, ["retail_count", "inst_count", "hot_count", "retail_cash_median"], `${path}.npcs`)
  const config = record(parsed.config, `${path}.config`)
  exact(config, ["commission_rate", "commission_min", "stamp_tax_rate", "default_limit", "st_limit", "price_cage_enabled", "lot_size", "starting_cash"], `${path}.config`)
  const params = record(parsed.strategy_params, `${path}.strategy_params`)
  exact(params, ["retail", "inst", "hot"], `${path}.strategy_params`)
  const retail = record(params.retail, `${path}.strategy_params.retail`)
  const inst = record(params.inst, `${path}.strategy_params.inst`)
  const hot = record(params.hot, `${path}.strategy_params.hot`)
  exact(retail, ["arrival_rate", "order_size_mean", "chase_prob"], `${path}.strategy_params.retail`)
  exact(inst, ["margin", "order_size"], `${path}.strategy_params.inst`)
  exact(hot, ["lookback", "trend_threshold", "order_size"], `${path}.strategy_params.hot`)
  return {
    company_system: parseCompanySystemConfig(parsed.company_system),
    report_frequency: parseReportFrequency(parsed.report_frequency, `${path}.report_frequency`),
    stocks: array(parsed.stocks, `${path}.stocks`).map((item, index) => stock(item, `${path}.stocks[${index}]`)),
    npcs: { retail_count: integer(npcs.retail_count, `${path}.npcs.retail_count`, 0), inst_count: integer(npcs.inst_count, `${path}.npcs.inst_count`, 0), hot_count: integer(npcs.hot_count, `${path}.npcs.hot_count`, 0), retail_cash_median: money(npcs.retail_cash_median, `${path}.npcs.retail_cash_median`) },
    config: { commission_rate: finite(config.commission_rate, `${path}.config.commission_rate`), commission_min: money(config.commission_min, `${path}.config.commission_min`), stamp_tax_rate: finite(config.stamp_tax_rate, `${path}.config.stamp_tax_rate`), default_limit: finite(config.default_limit, `${path}.config.default_limit`), st_limit: finite(config.st_limit, `${path}.config.st_limit`), price_cage_enabled: boolean(config.price_cage_enabled, `${path}.config.price_cage_enabled`), lot_size: integer(config.lot_size, `${path}.config.lot_size`, 1), starting_cash: money(config.starting_cash, `${path}.config.starting_cash`) },
    strategy_params: { retail: { arrival_rate: finite(retail.arrival_rate, `${path}.strategy_params.retail.arrival_rate`), order_size_mean: integer(retail.order_size_mean, `${path}.strategy_params.retail.order_size_mean`, 0), chase_prob: finite(retail.chase_prob, `${path}.strategy_params.retail.chase_prob`) }, inst: { margin: finite(inst.margin, `${path}.strategy_params.inst.margin`), order_size: integer(inst.order_size, `${path}.strategy_params.inst.order_size`, 1) }, hot: { lookback: integer(hot.lookback, `${path}.strategy_params.hot.lookback`, 2), trend_threshold: finite(hot.trend_threshold, `${path}.strategy_params.hot.trend_threshold`), order_size: integer(hot.order_size, `${path}.strategy_params.hot.order_size`, 1) } },
    ticks_per_day: integer(parsed.ticks_per_day, `${path}.ticks_per_day`, 1), auction_ticks: integer(parsed.auction_ticks, `${path}.auction_ticks`, 0), closing_auction_ticks: integer(parsed.closing_auction_ticks, `${path}.closing_auction_ticks`, 0), history_len: integer(parsed.history_len, `${path}.history_len`, 0), t1_enabled: boolean(parsed.t1_enabled, `${path}.t1_enabled`), float_allocation: floatAllocation(parsed.float_allocation, `${path}.float_allocation`), start_date: civilDate(parsed.start_date, `${path}.start_date`), simulation_policy_id: string(parsed.simulation_policy_id, `${path}.simulation_policy_id`), dividend_tax_mode: oneOf(parsed.dividend_tax_mode, `${path}.dividend_tax_mode`, ["IndividualPublicMarket", "Exempt"] as const),
  }
}

export function parseDailyCandle(value: unknown, path: string): DailyCandle {
  const parsed = record(value, path)
  const time = integer(parsed.time, `${path}.time`)
  try {
    candleDate(time)
  } catch (error) {
    throw new Error(`存档 ${path}.time 日期无效：${error instanceof Error ? error.message : String(error)}`)
  }
  const keys = "trade_stats" in parsed ? ["time", "open", "high", "low", "close", "volume", "trade_stats"] : ["time", "open", "high", "low", "close", "volume"]
  exact(parsed, keys, path)
  const stats = parsed.trade_stats === undefined ? undefined : nullable(parsed.trade_stats, `${path}.trade_stats`, (nested, statsPath) => {
    const result = record(nested, statsPath)
    exact(result, ["turnover_cents", "trade_count"], statsPath)
    let turnoverCents: string
    try {
      turnoverCents = parseTurnoverCents(result.turnover_cents, `${statsPath}.turnover_cents`)
    } catch (error) {
      if (!(error instanceof Error)) throw error
      throw new SaveSchemaError(`${statsPath}.turnover_cents`, error.message)
    }
    return { turnover_cents: turnoverCents, trade_count: integer(result.trade_count, `${statsPath}.trade_count`, 0) }
  })
  return stats === undefined ? { time: integer(parsed.time, `${path}.time`), open: money(parsed.open, `${path}.open`), high: money(parsed.high, `${path}.high`), low: money(parsed.low, `${path}.low`), close: money(parsed.close, `${path}.close`), volume: integer(parsed.volume, `${path}.volume`, 0) } : { time: integer(parsed.time, `${path}.time`), open: money(parsed.open, `${path}.open`), high: money(parsed.high, `${path}.high`), low: money(parsed.low, `${path}.low`), close: money(parsed.close, `${path}.close`), volume: integer(parsed.volume, `${path}.volume`, 0), trade_stats: stats }
}

export function parseSnapshot(value: unknown, path: string): Snapshot {
  const parsed = record(value, path)
  exact(parsed, ["seq", "tick", "day", "phase", "markets", "accounts", "daily_candles", "active_daily_candles"], path)
  return { seq: integer(parsed.seq, `${path}.seq`, 0), tick: integer(parsed.tick, `${path}.tick`, 0), day: integer(parsed.day, `${path}.day`, 0), phase: oneOf(parsed.phase, `${path}.phase`, phase), markets: map(parsed.markets, `${path}.markets`, stockCode, (item, itemPath) => { const market = record(item, itemPath); exact(market, ["last_price", "last_close", "best_bid", "best_ask", "bids", "asks", "cash_ex_reference_pending_trade", "day_market_activity", "last_cash_ex_reference"], itemPath); return { last_price: money(market.last_price, `${itemPath}.last_price`), last_close: money(market.last_close, `${itemPath}.last_close`), best_bid: nullable(market.best_bid, `${itemPath}.best_bid`, money), best_ask: nullable(market.best_ask, `${itemPath}.best_ask`, money), bids: array(market.bids, `${itemPath}.bids`).map((quote, index) => { const tuple = array(quote, `${itemPath}.bids[${index}]`); if (tuple.length !== 2) throw new Error(`存档 ${itemPath}.bids[${index}] 必须是二元组`); return [money(tuple[0], `${itemPath}.bids[${index}][0]`), integer(tuple[1], `${itemPath}.bids[${index}][1]`, 0)] }), asks: array(market.asks, `${itemPath}.asks`).map((quote, index) => { const tuple = array(quote, `${itemPath}.asks[${index}]`); if (tuple.length !== 2) throw new Error(`存档 ${itemPath}.asks[${index}] 必须是二元组`); return [money(tuple[0], `${itemPath}.asks[${index}][0]`), integer(tuple[1], `${itemPath}.asks[${index}][1]`, 0)] }), cash_ex_reference_pending_trade: boolean(market.cash_ex_reference_pending_trade, `${itemPath}.cash_ex_reference_pending_trade`), day_market_activity: boolean(market.day_market_activity, `${itemPath}.day_market_activity`), last_cash_ex_reference: nullable(market.last_cash_ex_reference, `${itemPath}.last_cash_ex_reference`, parseSaveExReferencePrice) } }), accounts: map(parsed.accounts, `${path}.accounts`, accountKey, (item, itemPath) => { const account = record(item, itemPath); exact(account, ["cash", "positions", "reserved_cash", "reserved_sell_qty"], itemPath); return { cash: money(account.cash, `${itemPath}.cash`), positions: map(account.positions, `${itemPath}.positions`, stockCode, (position, positionPath) => { const result = record(position, positionPath); exact(result, ["qty", "t1_locked", "invested_cents", "recovered_cents"], positionPath); return { qty: integer(result.qty, `${positionPath}.qty`, 0), t1_locked: integer(result.t1_locked, `${positionPath}.t1_locked`, 0), invested_cents: money(result.invested_cents, `${positionPath}.invested_cents`), recovered_cents: money(result.recovered_cents, `${positionPath}.recovered_cents`) } }), reserved_cash: money(account.reserved_cash, `${itemPath}.reserved_cash`), reserved_sell_qty: map(account.reserved_sell_qty, `${itemPath}.reserved_sell_qty`, stockCode, (qty, qtyPath) => integer(qty, qtyPath, 0)) } }), daily_candles: map(parsed.daily_candles, `${path}.daily_candles`, stockCode, (items, itemPath) => array(items, itemPath).map((item, index) => parseDailyCandle(item, `${itemPath}[${index}]`))), active_daily_candles: map(parsed.active_daily_candles, `${path}.active_daily_candles`, stockCode, parseDailyCandle) }
}

function parseSaveExReferencePrice(value: unknown, path: string): ExReferencePrice {
  const reference = record(value, path)
  exact(reference, ["ex_date", "reference_price"], path)
  const exDate = civilDate(reference.ex_date, `${path}.ex_date`)
  const year = Number(exDate.slice(0, 4))
  if (year < 1900 || year > 2199) throw new SaveSchemaError(`${path}.ex_date`, "除息锚点年份必须在 1900 至 2199 之间")
  const parsed = {
    ex_date: exDate,
    reference_price: money(reference.reference_price, `${path}.reference_price`),
  }
  if (BigInt(parsed.reference_price) <= 0n) throw new SaveSchemaError(`${path}.reference_price`, "除息参考价必须为正")
  return parsed
}
