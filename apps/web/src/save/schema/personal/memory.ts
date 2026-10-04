import { exact, integer, map, record } from "../primitives.ts"
import { accountKey, marketMinute, money, stockKey, type StringMap } from "./common.ts"

export type PersonalWatchlist = { readonly stocks: StringMap<WatchedStock>; readonly latest_attention_minute: string }
export type WatchedStock = { readonly last_observed_market_minute: string }
export type PersonalPriceMemory = { readonly stocks: StringMap<StockPriceMemory> }
export type StockPriceMemory = { readonly first_observed_minute: string; readonly first_observed_price: string; readonly last_observed_minute: string; readonly last_observed_price: string; readonly observed_high: string; readonly observed_low: string; readonly last_touched_minute: string }
export type PersonalHistoryReadLedger = { readonly stocks: StringMap<StockHistoryRead> }
export type StockHistoryRead = { readonly last_read_market_minute: string; readonly read_count: number }

export function parseWatchlists(value: unknown, path = "watchlists"): StringMap<PersonalWatchlist> { return map(value, path, accountKey, parseWatchlist) }
export function parsePriceMemories(value: unknown, path = "price_memories"): StringMap<PersonalPriceMemory> { return map(value, path, accountKey, parsePriceMemory) }
export function parseHistoryReads(value: unknown, path = "history_reads"): StringMap<PersonalHistoryReadLedger> { return map(value, path, accountKey, parseHistoryReadLedger) }

function parseWatchlist(value: unknown, path: string): PersonalWatchlist { const parsed = record(value, path); exact(parsed, ["stocks", "latest_attention_minute"], path); return { stocks: map(parsed.stocks, `${path}.stocks`, stockKey, parseWatchedStock), latest_attention_minute: marketMinute(parsed.latest_attention_minute, `${path}.latest_attention_minute`) } }
function parseWatchedStock(value: unknown, path: string): WatchedStock { const parsed = record(value, path); exact(parsed, ["last_observed_market_minute"], path); return { last_observed_market_minute: marketMinute(parsed.last_observed_market_minute, `${path}.last_observed_market_minute`) } }
function parsePriceMemory(value: unknown, path: string): PersonalPriceMemory { const parsed = record(value, path); exact(parsed, ["stocks"], path); return { stocks: map(parsed.stocks, `${path}.stocks`, stockKey, parseStockPriceMemory) } }
function parseStockPriceMemory(value: unknown, path: string): StockPriceMemory { const parsed = record(value, path); exact(parsed, ["first_observed_minute", "first_observed_price", "last_observed_minute", "last_observed_price", "observed_high", "observed_low", "last_touched_minute"], path); return { first_observed_minute: marketMinute(parsed.first_observed_minute, `${path}.first_observed_minute`), first_observed_price: money(parsed.first_observed_price, `${path}.first_observed_price`), last_observed_minute: marketMinute(parsed.last_observed_minute, `${path}.last_observed_minute`), last_observed_price: money(parsed.last_observed_price, `${path}.last_observed_price`), observed_high: money(parsed.observed_high, `${path}.observed_high`), observed_low: money(parsed.observed_low, `${path}.observed_low`), last_touched_minute: marketMinute(parsed.last_touched_minute, `${path}.last_touched_minute`) } }
function parseHistoryReadLedger(value: unknown, path: string): PersonalHistoryReadLedger { const parsed = record(value, path); exact(parsed, ["stocks"], path); return { stocks: map(parsed.stocks, `${path}.stocks`, stockKey, parseStockHistoryRead) } }
function parseStockHistoryRead(value: unknown, path: string): StockHistoryRead { const parsed = record(value, path); exact(parsed, ["last_read_market_minute", "read_count"], path); return { last_read_market_minute: marketMinute(parsed.last_read_market_minute, `${path}.last_read_market_minute`), read_count: integer(parsed.read_count, `${path}.read_count`, 1) } }
