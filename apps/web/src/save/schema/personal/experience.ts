import { array, boolean, civilDate, exact, integer, map, nullable, record } from "../primitives.ts"
import { accountKey, marketMinute, money, stockKey, type StringMap } from "./common.ts"

export type CivilInstant = { readonly date: string; readonly second_of_day: number }
export type ExperienceMoment = { readonly civil_date: string; readonly market_minute: string; readonly trading_day: string }
export type ExperienceFeedback = {
  readonly latest_moment: ExperienceMoment | null
  readonly failure_events: readonly FailureEventRecord[]
  readonly stocks: StringMap<HoldingEpoch>
  readonly exit_records: readonly ExitRecord[]
}
export type FailureEventRecord = { readonly code: string; readonly order_id: string | null; readonly moment: ExperienceMoment }
export type HoldingEpoch = { readonly entry_moment: ExperienceMoment; readonly last_own_observation: OwnObservation | null }
export type OwnObservation = { readonly price: number; readonly moment: ExperienceMoment }
export type ExitRecord = { readonly code: string; readonly cooldown_until_market_minute: string; readonly realized_profit: boolean; readonly moment: ExperienceMoment }
export type RetailExperienceState = {
  readonly reference_equity: number | null
  readonly peak_equity: number | null
  readonly consecutive_failed_buys: number
  readonly stocks: StringMap<RetailStockExperience>
  readonly feedback?: ExperienceFeedback
}
export type RetailStockExperience = {
  readonly entry_reference_price: number | null
  readonly peak_price_since_entry: number | null
  readonly last_buy_price: number | null
  readonly adverse_move_recorded: boolean
  readonly last_buy_order_id: string | null
  readonly last_sell_order_id: string | null
  readonly last_trade_market_minute: string
  readonly last_observed_market_minute: string
  readonly cooldown_until_market_minute: string | null
}

export function parseCivilInstant(value: unknown, path: string): CivilInstant {
  const parsed = record(value, path)
  exact(parsed, ["date", "second_of_day"], path)
  const second_of_day = integer(parsed.second_of_day, `${path}.second_of_day`, 0)
  if (second_of_day >= 86_400) throw new Error(`存档 ${path}.second_of_day 必须小于 86400`)
  return { date: parseCivilDate(parsed.date, `${path}.date`), second_of_day }
}

export function parseRetailExperienceStates(value: unknown, path = "retail_experience"): StringMap<RetailExperienceState> {
  return map(value, path, accountKey, parseRetailExperienceState)
}

export function parseRetailExperienceState(value: unknown, path: string): RetailExperienceState {
  const parsed = record(value, path)
  const hasFeedback = "feedback" in parsed
  exact(parsed, hasFeedback ? ["reference_equity", "peak_equity", "consecutive_failed_buys", "stocks", "feedback"] : ["reference_equity", "peak_equity", "consecutive_failed_buys", "stocks"], path)
  return {
    reference_equity: nullable(parsed.reference_equity, `${path}.reference_equity`, money),
    peak_equity: nullable(parsed.peak_equity, `${path}.peak_equity`, money),
    consecutive_failed_buys: integer(parsed.consecutive_failed_buys, `${path}.consecutive_failed_buys`, 0),
    stocks: map(parsed.stocks, `${path}.stocks`, stockKey, parseRetailStockExperience),
    ...(hasFeedback ? { feedback: parseExperienceFeedback(parsed.feedback, `${path}.feedback`) } : {}),
  }
}

export function parseExperienceFeedback(value: unknown, path: string): ExperienceFeedback {
  const parsed = record(value, path)
  exact(parsed, ["latest_moment", "failure_events", "stocks", "exit_records"], path)
  return {
    latest_moment: nullable(parsed.latest_moment, `${path}.latest_moment`, parseExperienceMoment),
    failure_events: array(parsed.failure_events, `${path}.failure_events`).map((entry, index) => parseFailureEvent(entry, `${path}.failure_events[${index}]`)),
    stocks: map(parsed.stocks, `${path}.stocks`, stockKey, parseHoldingEpoch),
    exit_records: array(parsed.exit_records, `${path}.exit_records`).map((entry, index) => parseExitRecord(entry, `${path}.exit_records[${index}]`)),
  }
}

function parseRetailStockExperience(value: unknown, path: string): RetailStockExperience {
  const parsed = record(value, path)
  exact(parsed, ["entry_reference_price", "peak_price_since_entry", "last_buy_price", "adverse_move_recorded", "last_buy_order_id", "last_sell_order_id", "last_trade_market_minute", "last_observed_market_minute", "cooldown_until_market_minute"], path)
  return { entry_reference_price: nullable(parsed.entry_reference_price, `${path}.entry_reference_price`, money), peak_price_since_entry: nullable(parsed.peak_price_since_entry, `${path}.peak_price_since_entry`, money), last_buy_price: nullable(parsed.last_buy_price, `${path}.last_buy_price`, money), adverse_move_recorded: boolean(parsed.adverse_move_recorded, `${path}.adverse_move_recorded`), last_buy_order_id: nullable(parsed.last_buy_order_id, `${path}.last_buy_order_id`, marketMinute), last_sell_order_id: nullable(parsed.last_sell_order_id, `${path}.last_sell_order_id`, marketMinute), last_trade_market_minute: marketMinute(parsed.last_trade_market_minute, `${path}.last_trade_market_minute`), last_observed_market_minute: marketMinute(parsed.last_observed_market_minute, `${path}.last_observed_market_minute`), cooldown_until_market_minute: nullable(parsed.cooldown_until_market_minute, `${path}.cooldown_until_market_minute`, marketMinute) }
}

function parseExperienceMoment(value: unknown, path: string): ExperienceMoment {
  const parsed = record(value, path)
  exact(parsed, ["civil_date", "market_minute", "trading_day"], path)
  return { civil_date: parseCivilDate(parsed.civil_date, `${path}.civil_date`), market_minute: marketMinute(parsed.market_minute, `${path}.market_minute`), trading_day: marketMinute(parsed.trading_day, `${path}.trading_day`) }
}

function parseFailureEvent(value: unknown, path: string): FailureEventRecord {
  const parsed = record(value, path)
  exact(parsed, ["code", "order_id", "moment"], path)
  return { code: parseStock(parsed.code, `${path}.code`), order_id: nullable(parsed.order_id, `${path}.order_id`, marketMinute), moment: parseExperienceMoment(parsed.moment, `${path}.moment`) }
}

function parseHoldingEpoch(value: unknown, path: string): HoldingEpoch {
  const parsed = record(value, path)
  exact(parsed, ["entry_moment", "last_own_observation"], path)
  return { entry_moment: parseExperienceMoment(parsed.entry_moment, `${path}.entry_moment`), last_own_observation: nullable(parsed.last_own_observation, `${path}.last_own_observation`, parseOwnObservation) }
}

function parseOwnObservation(value: unknown, path: string): OwnObservation {
  const parsed = record(value, path)
  exact(parsed, ["price", "moment"], path)
  return { price: money(parsed.price, `${path}.price`), moment: parseExperienceMoment(parsed.moment, `${path}.moment`) }
}

function parseExitRecord(value: unknown, path: string): ExitRecord {
  const parsed = record(value, path)
  exact(parsed, ["code", "cooldown_until_market_minute", "realized_profit", "moment"], path)
  return { code: parseStock(parsed.code, `${path}.code`), cooldown_until_market_minute: marketMinute(parsed.cooldown_until_market_minute, `${path}.cooldown_until_market_minute`), realized_profit: boolean(parsed.realized_profit, `${path}.realized_profit`), moment: parseExperienceMoment(parsed.moment, `${path}.moment`) }
}

function parseStock(value: unknown, path: string): string {
  if (typeof value !== "string") throw new Error(`存档 ${path} 必须是字符串`)
  stockKey(value, path)
  return value
}

function parseCivilDate(value: unknown, path: string): string {
  const date = civilDate(value, path)
  const [yearText, monthText, dayText] = date.split("-")
  if (yearText === undefined || monthText === undefined || dayText === undefined) throw new Error(`存档 ${path} 必须是有效公历日期`)
  const year = BigInt(yearText)
  if (year < 1900n || year > 2199n) throw new Error(`存档 ${path} 必须在公历验证窗口内`)
  const monthDays: { readonly [key: string]: string } = {
    "01": "31", "02": year % 4n === 0n && (year % 100n !== 0n || year % 400n === 0n) ? "29" : "28", "03": "31", "04": "30", "05": "31", "06": "30", "07": "31", "08": "31", "09": "30", "10": "31", "11": "30", "12": "31",
  }
  const maximum = monthDays[monthText]
  if (maximum === undefined || BigInt(dayText) === 0n || BigInt(dayText) > BigInt(maximum)) throw new Error(`存档 ${path} 必须是有效公历日期`)
  return date
}
