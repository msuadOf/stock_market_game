import assert from "node:assert/strict"
import test from "node:test"
import { currentSaveFixture, withSettledDay } from "../current-save-fixture.ts"
import { parseSaveJson } from "../save-schema.ts"
import { scanArchiveRoot } from "./archive-root-scan.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"
import { MarketShardValidation } from "./market-shard-validation.ts"

const fixture = () => withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")
const base = parseSaveJson(JSON.stringify(fixture()))
const context = { stocks: [{ id: "0", tick: "1" }], start_date: base.setup.start_date, settled_through: base.civil_clock.settled_through, auction_ticks: base.setup.auction_ticks, closing_auction_ticks: base.setup.closing_auction_ticks, candles: base.snapshot.daily_candles }
const bar = { minute_of_day: 570, phase: "Continuous", open: "1120", high: "1120", low: "1120", close: "1120", volume_shares: "100", turnover_cents: "112000", trade_count: "1" }
const day = { date: "2030-01-01", securities: { "0": { trading: true, bars: [] } } }
async function* chunks(text: string): AsyncIterable<string> { for (let index = 0; index < text.length; index += 29) yield text.slice(index, index + 29) }
async function grouped(text: string) {
  const ledger = new SchemaFailureLedger(), market = new MarketShardValidation(ledger, context)
  const scan = await scanArchiveRoot(chunks(text), market.options())
  const summary = market.finish(scan)
  ledger.throwIfAny()
  return summary
}
function errorOf(operation: () => unknown): string {
  try { operation() } catch (error) { assert.ok(error instanceof Error); return error.message }
  return "ACCEPTED"
}

test("订单、活动分钟及保留日逐证券/逐日验证，保留日终摘要", { timeout: 10000 }, async () => {
  const result = await grouped(JSON.stringify(fixture()))
  assert.equal(result.retainedDays, 1)
  assert.equal(result.hasRestingOrders, false)
  assert.equal(result.hasActiveMinutes, false)
  assert.equal(result.schemaValidated, false)
})

test("resting/filled及active minute原map形状、StockId和leaf首错不变", { timeout: 10000 }, async () => {
  for (const [field, value] of [
    ["resting_orders", null], ["resting_orders", 1], ["resting_orders", { "0": [{}] }], ["filled_orders", false], ["filled_orders", { "0": [{}] }],
    ["market_minute_bars", null], ["market_minute_bars", 1], ["market_minute_bars", { bad: [] }], ["market_minute_bars", { "1": [] }], ["market_minute_bars", { "0": [{}] }], ["market_minute_bars", { "0": [bar, bar] }],
  ] as const) {
    const text = JSON.stringify({ ...fixture(), [field]: value })
    const expected = errorOf(() => parseSaveJson(text))
    assert.notEqual(expected, "ACCEPTED")
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
  }
})

test("retained历史连续覆盖、证券集合、休市及日K互证保持原codec", { timeout: 10000 }, async () => {
  for (const value of [null, 1, {}, [], [{}], [{ ...day, date: "2030-01-02" }], [{ ...day, securities: {} }], [{ ...day, securities: { "0": { trading: "true", bars: [] } } }], [{ ...day, securities: { "0": { trading: false, bars: [bar] } } }], [{ ...day, securities: { "0": { trading: true, bars: [bar] } } }]]) {
    const text = JSON.stringify({ ...fixture(), retained_market_days: value })
    const expected = errorOf(() => parseSaveJson(text))
    assert.notEqual(expected, "ACCEPTED")
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
  }
})

test("完整retained leaf/schema检查先于全部candles cross-check", { timeout: 10000 }, async () => {
  const text = JSON.stringify({ ...fixture(), retained_market_days: [{ ...day, securities: { "0": { trading: true, bars: [bar] } } }, {}] })
  await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
})

test("合法日内订单和分钟仍先完整校验，再显式记入日终摘要", { timeout: 10000 }, async () => {
  const order = { id: 1, side: "Buy", price: "1120", qty: 100, original_qty: 100, filled_qty: 0, filled_value: "0", owner: "0", seq: 0 }
  const text = JSON.stringify({ ...fixture(), resting_orders: { "0": [order] }, filled_orders: { "0": [{ id: 2, owner: "1" }] }, market_minute_bars: { "0": [bar] } })
  assert.doesNotThrow(() => parseSaveJson(text))
  const result = await grouped(text)
  assert.equal(result.hasRestingOrders, true)
  assert.equal(result.hasActiveMinutes, true)
})
