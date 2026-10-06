import { money as parseCanonicalMoney } from "./primitives.ts"
import type { AccountId } from "../../types/generated/AccountId.ts"
import type { DailyCandle } from "../../types/generated/DailyCandle.ts"
import type { Money } from "../../types/generated/Money.ts"
import type { PositionSnap } from "../../types/generated/PositionSnap.ts"
import type { StockCode } from "../../types/generated/StockCode.ts"
import type { ExReferencePrice } from "./company/ex-reference-price.ts"
import { parseDailyCandle } from "./market.ts"
import { stockKey } from "./personal/common.ts"
import { array, boolean, civilDate, exact, integer, map, record, SaveSchemaError, accountKey } from "./primitives.ts"

type SaveMarket = { readonly last_price: Money; readonly last_close: Money; readonly cash_ex_reference_pending_trade: boolean; readonly day_market_activity: boolean; readonly last_cash_ex_reference: ExReferencePrice | null }
type SaveAccount = { readonly cash: Money; readonly positions: Readonly<Record<StockCode, PositionSnap>> }

export type SaveSnapshot = {
  readonly seq: number
  readonly tick: number
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
    invested_cents: money(parsed.invested_cents, `${path}.invested_cents`),
    recovered_cents: money(parsed.recovered_cents, `${path}.recovered_cents`),
  }
  if (position.qty === 0 || position.t1_locked > position.qty || BigInt(position.invested_cents) < 0n || BigInt(position.recovered_cents) < 0n) {
    throw new Error(`存档 ${path} 持仓不满足数量、T+1 锁定或成本基础约束`)
  }
  return position
}

function money(value: unknown, path: string): string {
  return parseCanonicalMoney(value, path)
}

export function parseSaveSnapshot(value: unknown, path: string): SaveSnapshot {
  const parsed = record(value, path)
  exact(parsed, ["seq", "tick", "markets", "accounts", "daily_candles", "active_daily_candles"], path)
  const markets = map(parsed.markets, `${path}.markets`, stockKey, (item, itemPath) => {
    const market = record(item, itemPath)
    exact(market, ["last_price", "last_close", "cash_ex_reference_pending_trade", "day_market_activity", "last_cash_ex_reference"], itemPath)
    let last_cash_ex_reference: ExReferencePrice | null = null
    if (market.last_cash_ex_reference !== null) {
      const anchor = record(market.last_cash_ex_reference, `${itemPath}.last_cash_ex_reference`)
      exact(anchor, ["ex_date", "reference_price"], `${itemPath}.last_cash_ex_reference`)
      last_cash_ex_reference = {
        ex_date: civilDate(anchor.ex_date, `${itemPath}.last_cash_ex_reference.ex_date`),
        reference_price: money(anchor.reference_price, `${itemPath}.last_cash_ex_reference.reference_price`),
      }
      if (BigInt(last_cash_ex_reference.reference_price) <= 0n) throw new SaveSchemaError(`${itemPath}.last_cash_ex_reference.reference_price`, "除息参考价必须为正")
    }
    return {
      last_price: money(market.last_price, `${itemPath}.last_price`),
      last_close: money(market.last_close, `${itemPath}.last_close`),
      cash_ex_reference_pending_trade: boolean(market.cash_ex_reference_pending_trade, `${itemPath}.cash_ex_reference_pending_trade`),
      day_market_activity: boolean(market.day_market_activity, `${itemPath}.day_market_activity`),
      last_cash_ex_reference,
    }
  })
  const accounts = map(parsed.accounts, `${path}.accounts`, accountKey, (item, itemPath) => {
    const account = record(item, itemPath)
    exact(account, ["cash", "positions"], itemPath)
    const cash = money(account.cash, `${itemPath}.cash`)
    if (BigInt(cash) < 0n) throw new Error(`存档 ${itemPath}.cash 不能为负数`)
    return {
      cash,
      positions: map(account.positions, `${itemPath}.positions`, stockKey, parsePosition),
    }
  })
  return {
    seq: integer(parsed.seq, `${path}.seq`, 0),
    tick: integer(parsed.tick, `${path}.tick`, 0),
    markets,
    accounts,
    daily_candles: map(parsed.daily_candles, `${path}.daily_candles`, stockKey, (items, itemPath) => array(items, itemPath).map((item, index) => parseDailyCandle(item, `${itemPath}[${index}]`))),
    active_daily_candles: map(parsed.active_daily_candles, `${path}.active_daily_candles`, stockKey, parseDailyCandle),
  } as SaveSnapshot
}

export function validateCashExReferenceFacts(snapshot: SaveSnapshot, setupStocks: readonly { readonly code: string; readonly tick: string }[], currentDate: string): void {
  for (const [code, market] of Object.entries(snapshot.markets)) {
    const path = `snapshot.markets.${code}`
    const anchor = market.last_cash_ex_reference
    if (anchor !== null) {
      if (anchor.ex_date > currentDate) throw new SaveSchemaError(`${path}.last_cash_ex_reference.ex_date`, "除息事实日期晚于当前存档日")
      const tick = setupStocks.find(stock => stock.code === code)?.tick
      if (tick === undefined || BigInt(tick) <= 0n || BigInt(anchor.reference_price) % BigInt(tick) !== 0n) throw new SaveSchemaError(`${path}.last_cash_ex_reference.reference_price`, "除息参考价必须符合证券最小价位单位")
    }
    if (market.cash_ex_reference_pending_trade && (anchor === null || anchor.ex_date > currentDate || anchor.reference_price !== market.last_close)) throw new SaveSchemaError(`${path}.cash_ex_reference_pending_trade`, "待首笔交易的除息参考价必须已安装、未晚于当前日且等于昨收价")
  }
}
