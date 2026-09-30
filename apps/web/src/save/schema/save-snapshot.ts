import type { AccountId } from "../../types/generated/AccountId.ts"
import type { DailyCandle } from "../../types/generated/DailyCandle.ts"
import type { Money } from "../../types/generated/Money.ts"
import type { PositionSnap } from "../../types/generated/PositionSnap.ts"
import type { StockCode } from "../../types/generated/StockCode.ts"
import type { TradingPhase } from "../../types/generated/TradingPhase.ts"
import { TRADING_PHASES } from "../../host/protocol/wire-values.ts"
import { parseDailyCandle } from "./market.ts"
import { stockKey } from "./personal/common.ts"
import { array, exact, integer, map, oneOf, record, safeIntegerKey } from "./primitives.ts"

type SaveMarket = { readonly last_price: Money; readonly last_close: Money }
type SaveAccount = { readonly cash: Money; readonly positions: Readonly<Record<StockCode, PositionSnap>> }

export type SaveSnapshot = {
  readonly seq: number
  readonly tick: number
  readonly day: number
  readonly phase: TradingPhase
  readonly markets: Readonly<Record<StockCode, SaveMarket>>
  readonly accounts: Readonly<Record<AccountId, SaveAccount>>
  readonly daily_candles: Readonly<Record<StockCode, readonly DailyCandle[]>>
  readonly active_daily_candles: Readonly<Record<StockCode, DailyCandle>>
}

function parsePosition(value: unknown, path: string): PositionSnap {
  const parsed = record(value, path)
  exact(parsed, ["qty", "t1_locked", "invested_cents", "recovered_cents"], path)
  const position = {
    qty: integer(parsed.qty, `${path}.qty`, 0),
    t1_locked: integer(parsed.t1_locked, `${path}.t1_locked`, 0),
    invested_cents: integer(parsed.invested_cents, `${path}.invested_cents`),
    recovered_cents: integer(parsed.recovered_cents, `${path}.recovered_cents`),
  }
  if (position.qty === 0 || position.t1_locked > position.qty || position.invested_cents < 0 || position.recovered_cents < 0) {
    throw new Error(`存档 ${path} 持仓不满足数量、T+1 锁定或成本基础约束`)
  }
  return position
}

function money(value: unknown, path: string): Money {
  return integer(value, path)
}

export function parseSaveSnapshot(value: unknown, path: string): SaveSnapshot {
  const parsed = record(value, path)
  exact(parsed, ["seq", "tick", "day", "phase", "markets", "accounts", "daily_candles", "active_daily_candles"], path)
  const markets = map(parsed.markets, `${path}.markets`, stockKey, (item, itemPath) => {
    const market = record(item, itemPath)
    exact(market, ["last_price", "last_close"], itemPath)
    return {
      last_price: money(market.last_price, `${itemPath}.last_price`),
      last_close: money(market.last_close, `${itemPath}.last_close`),
    }
  })
  const accounts = map(parsed.accounts, `${path}.accounts`, safeIntegerKey, (item, itemPath) => {
    const account = record(item, itemPath)
    exact(account, ["cash", "positions"], itemPath)
    const cash = money(account.cash, `${itemPath}.cash`)
    if (cash < 0) throw new Error(`存档 ${itemPath}.cash 不能为负数`)
    return {
      cash,
      positions: map(account.positions, `${itemPath}.positions`, stockKey, parsePosition),
    }
  })
  return {
    seq: integer(parsed.seq, `${path}.seq`, 0),
    tick: integer(parsed.tick, `${path}.tick`, 0),
    day: integer(parsed.day, `${path}.day`, 0),
    phase: oneOf(parsed.phase, `${path}.phase`, TRADING_PHASES),
    markets,
    accounts,
    daily_candles: map(parsed.daily_candles, `${path}.daily_candles`, stockKey, (items, itemPath) => array(items, itemPath).map((item, index) => parseDailyCandle(item, `${itemPath}[${index}]`))),
    active_daily_candles: map(parsed.active_daily_candles, `${path}.active_daily_candles`, stockKey, parseDailyCandle),
  } as SaveSnapshot
}
