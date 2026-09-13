import type { FloatAllocation, SessionSetup, Snapshot } from "../../types/engine"
import type { DailyCandle } from "../../types/generated/DailyCandle"
import { array, boolean, civilDate, decimal, exact, finite, integer, map, nullable, oneOf, record, string } from "./primitives.ts"

const exchange = ["Shanghai", "Shenzhen"] as const
const category = ["MainBoard", "StMainBoard", "ChiNext"] as const
const phase = ["CallAuction", "PreOpen", "ClosingAuction", "Continuous"] as const

function stockCode(value: string, path: string): void {
  if (value.length === 0) throw new Error(`存档 ${path} 股票代码不能为空`)
}

function money(value: unknown, path: string): number {
  return integer(value, path)
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
  if (value === "Random") return value
  const parsed = record(value, path)
  exact(parsed, ["ByKind"], path)
  const byKind = record(parsed.ByKind, `${path}.ByKind`)
  exact(byKind, ["retail", "inst", "hot"], `${path}.ByKind`)
  return { ByKind: { retail: finite(byKind.retail, `${path}.ByKind.retail`), inst: finite(byKind.inst, `${path}.ByKind.inst`), hot: finite(byKind.hot, `${path}.ByKind.hot`) } }
}

export function parseSetup(value: unknown, path: string): SessionSetup {
  const parsed = record(value, path)
  exact(parsed, ["stocks", "npcs", "config", "strategy_params", "ticks_per_day", "auction_ticks", "closing_auction_ticks", "history_len", "t1_enabled", "float_allocation", "start_date", "simulation_policy_id"], path)
  const npcs = record(parsed.npcs, `${path}.npcs`)
  exact(npcs, ["retail_count", "inst_count", "hot_count", "retail_cash_median"], `${path}.npcs`)
  const config = record(parsed.config, `${path}.config`)
  exact(config, ["commission_rate", "commission_min", "stamp_tax_rate", "default_limit", "st_limit", "lot_size", "starting_cash"], `${path}.config`)
  const params = record(parsed.strategy_params, `${path}.strategy_params`)
  exact(params, ["retail", "inst", "hot"], `${path}.strategy_params`)
  const retail = record(params.retail, `${path}.strategy_params.retail`)
  const inst = record(params.inst, `${path}.strategy_params.inst`)
  const hot = record(params.hot, `${path}.strategy_params.hot`)
  exact(retail, ["arrival_rate", "order_size_mean", "chase_prob", "tick_cents"], `${path}.strategy_params.retail`)
  exact(inst, ["margin", "order_size"], `${path}.strategy_params.inst`)
  exact(hot, ["lookback", "trend_threshold", "order_size"], `${path}.strategy_params.hot`)
  return {
    stocks: array(parsed.stocks, `${path}.stocks`).map((item, index) => stock(item, `${path}.stocks[${index}]`)),
    npcs: { retail_count: integer(npcs.retail_count, `${path}.npcs.retail_count`, 0), inst_count: integer(npcs.inst_count, `${path}.npcs.inst_count`, 0), hot_count: integer(npcs.hot_count, `${path}.npcs.hot_count`, 0), retail_cash_median: money(npcs.retail_cash_median, `${path}.npcs.retail_cash_median`) },
    config: { commission_rate: finite(config.commission_rate, `${path}.config.commission_rate`), commission_min: money(config.commission_min, `${path}.config.commission_min`), stamp_tax_rate: finite(config.stamp_tax_rate, `${path}.config.stamp_tax_rate`), default_limit: finite(config.default_limit, `${path}.config.default_limit`), st_limit: finite(config.st_limit, `${path}.config.st_limit`), lot_size: integer(config.lot_size, `${path}.config.lot_size`, 1), starting_cash: money(config.starting_cash, `${path}.config.starting_cash`) },
    strategy_params: { retail: { arrival_rate: finite(retail.arrival_rate, `${path}.strategy_params.retail.arrival_rate`), order_size_mean: integer(retail.order_size_mean, `${path}.strategy_params.retail.order_size_mean`, 0), chase_prob: finite(retail.chase_prob, `${path}.strategy_params.retail.chase_prob`), tick_cents: integer(retail.tick_cents, `${path}.strategy_params.retail.tick_cents`, 1) }, inst: { margin: finite(inst.margin, `${path}.strategy_params.inst.margin`), order_size: integer(inst.order_size, `${path}.strategy_params.inst.order_size`, 1) }, hot: { lookback: integer(hot.lookback, `${path}.strategy_params.hot.lookback`, 2), trend_threshold: finite(hot.trend_threshold, `${path}.strategy_params.hot.trend_threshold`), order_size: integer(hot.order_size, `${path}.strategy_params.hot.order_size`, 1) } },
    ticks_per_day: integer(parsed.ticks_per_day, `${path}.ticks_per_day`, 1), auction_ticks: integer(parsed.auction_ticks, `${path}.auction_ticks`, 0), closing_auction_ticks: integer(parsed.closing_auction_ticks, `${path}.closing_auction_ticks`, 0), history_len: integer(parsed.history_len, `${path}.history_len`, 0), t1_enabled: boolean(parsed.t1_enabled, `${path}.t1_enabled`), float_allocation: floatAllocation(parsed.float_allocation, `${path}.float_allocation`), start_date: civilDate(parsed.start_date, `${path}.start_date`), simulation_policy_id: string(parsed.simulation_policy_id, `${path}.simulation_policy_id`),
  }
}

function candle(value: unknown, path: string): DailyCandle {
  const parsed = record(value, path)
  const keys = "trade_stats" in parsed ? ["time", "open", "high", "low", "close", "volume", "trade_stats"] : ["time", "open", "high", "low", "close", "volume"]
  exact(parsed, keys, path)
  const stats = parsed.trade_stats === undefined ? undefined : nullable(parsed.trade_stats, `${path}.trade_stats`, (nested, statsPath) => {
    const result = record(nested, statsPath)
    exact(result, ["turnover_cents", "trade_count"], statsPath)
    return { turnover_cents: decimal(result.turnover_cents, `${statsPath}.turnover_cents`), trade_count: integer(result.trade_count, `${statsPath}.trade_count`, 0) }
  })
  return stats === undefined ? { time: integer(parsed.time, `${path}.time`), open: money(parsed.open, `${path}.open`), high: money(parsed.high, `${path}.high`), low: money(parsed.low, `${path}.low`), close: money(parsed.close, `${path}.close`), volume: integer(parsed.volume, `${path}.volume`, 0) } : { time: integer(parsed.time, `${path}.time`), open: money(parsed.open, `${path}.open`), high: money(parsed.high, `${path}.high`), low: money(parsed.low, `${path}.low`), close: money(parsed.close, `${path}.close`), volume: integer(parsed.volume, `${path}.volume`, 0), trade_stats: stats }
}

export function parseSnapshot(value: unknown, path: string): Snapshot {
  const parsed = record(value, path)
  exact(parsed, ["seq", "tick", "day", "phase", "markets", "accounts", "daily_candles", "active_daily_candles"], path)
  return { seq: integer(parsed.seq, `${path}.seq`, 0), tick: integer(parsed.tick, `${path}.tick`, 0), day: integer(parsed.day, `${path}.day`, 0), phase: oneOf(parsed.phase, `${path}.phase`, phase), markets: map(parsed.markets, `${path}.markets`, stockCode, (item, itemPath) => { const market = record(item, itemPath); exact(market, ["last_price", "last_close", "best_bid", "best_ask", "bids", "asks"], itemPath); return { last_price: money(market.last_price, `${itemPath}.last_price`), last_close: money(market.last_close, `${itemPath}.last_close`), best_bid: nullable(market.best_bid, `${itemPath}.best_bid`, money), best_ask: nullable(market.best_ask, `${itemPath}.best_ask`, money), bids: array(market.bids, `${itemPath}.bids`).map((quote, index) => { const tuple = array(quote, `${itemPath}.bids[${index}]`); if (tuple.length !== 2) throw new Error(`存档 ${itemPath}.bids[${index}] 必须是二元组`); return [money(tuple[0], `${itemPath}.bids[${index}][0]`), integer(tuple[1], `${itemPath}.bids[${index}][1]`, 0)] }), asks: array(market.asks, `${itemPath}.asks`).map((quote, index) => { const tuple = array(quote, `${itemPath}.asks[${index}]`); if (tuple.length !== 2) throw new Error(`存档 ${itemPath}.asks[${index}] 必须是二元组`); return [money(tuple[0], `${itemPath}.asks[${index}][0]`), integer(tuple[1], `${itemPath}.asks[${index}][1]`, 0)] }) } }), accounts: map(parsed.accounts, `${path}.accounts`, (key, keyPath) => { decimal(key, keyPath) }, (item, itemPath) => { const account = record(item, itemPath); exact(account, ["cash", "positions", "reserved_cash", "reserved_sell_qty"], itemPath); return { cash: money(account.cash, `${itemPath}.cash`), positions: map(account.positions, `${itemPath}.positions`, stockCode, (position, positionPath) => { const result = record(position, positionPath); exact(result, ["qty", "t1_locked", "invested_cents", "recovered_cents"], positionPath); return { qty: integer(result.qty, `${positionPath}.qty`, 0), t1_locked: integer(result.t1_locked, `${positionPath}.t1_locked`, 0), invested_cents: integer(result.invested_cents, `${positionPath}.invested_cents`), recovered_cents: integer(result.recovered_cents, `${positionPath}.recovered_cents`) } }), reserved_cash: money(account.reserved_cash, `${itemPath}.reserved_cash`), reserved_sell_qty: map(account.reserved_sell_qty, `${itemPath}.reserved_sell_qty`, stockCode, (qty, qtyPath) => integer(qty, qtyPath, 0)) } }), daily_candles: map(parsed.daily_candles, `${path}.daily_candles`, stockCode, (items, itemPath) => array(items, itemPath).map((item, index) => candle(item, `${itemPath}[${index}]`))), active_daily_candles: map(parsed.active_daily_candles, `${path}.active_daily_candles`, stockCode, candle) }
}
