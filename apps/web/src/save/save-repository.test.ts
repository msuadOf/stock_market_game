import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"
import { LocalStorageSaveRepository } from "./save-repository.ts"
import { parseSaveSlot } from "./save-schema.ts"

const maturePath = "/home/baiyifan/.claude/tmp/opencode/task29-save.json"
const mature = parseSaveSlot(JSON.parse(readFileSync(maturePath, "utf8")))

function repositoryFor(value: unknown): LocalStorageSaveRepository {
  return new LocalStorageSaveRepository({ getItem: () => JSON.stringify(value), setItem: () => undefined })
}

test("local save repository preserves the complete mature Rust save", () => {
  let stored: string | null = null
  const repository = new LocalStorageSaveRepository({ getItem: () => stored, setItem: (_key, value) => { stored = value } })
  repository.save(mature)
  assert.deepEqual(repository.load(), mature)
})

test("local save repository rejects malformed persisted JSON", () => {
  const repository = new LocalStorageSaveRepository({ getItem: () => "{", setItem: () => undefined })
  assert.throws(() => repository.load(), /存档不是合法 JSON/)
})

test("local save repository rejects unknown root fields without schema migration", () => {
  assert.throws(() => repositoryFor({ ...mature, schema_version: 1 }).load(), /schema_version/)
})

test("local save repository rejects missing required K7 state", () => {
  const { public_library: _removed, ...missing } = mature
  assert.throws(() => repositoryFor(missing).load(), /public_library/)
})

test("local save repository rejects numeric lossless values without coercion", () => {
  assert.throws(() => repositoryFor({ ...mature, seed: 9_007_199_254_740_992 }).load(), /seed/)
})

test("local save repository rejects invalid tagged intents", () => {
  assert.throws(() => repositoryFor({ ...mature, pending_player: [[1, { RetiredIntent: { code: "600101" } }]] }).load(), /pending_player\[0\]\[1\]/)
})

test("local save repository rejects malformed civil dates", () => {
  assert.throws(() => repositoryFor({ ...mature, setup: { ...mature.setup, start_date: "2030-02-30" } }).load(), /start_date/)
})

test("local save repository does not rewrite explicit A-share stock rules", () => {
  const stock = mature.setup.stocks[0]
  if (stock === undefined) throw new Error("mature save must contain stocks")
  const setup = { ...mature.setup, stocks: [{ ...stock, category: "StMainBoard" as const, limit_pct: 0.05 }] }
  const loaded = repositoryFor({ ...mature, setup }).load()
  assert.equal(loaded?.setup.stocks[0]?.category, "StMainBoard")
  assert.equal(loaded?.setup.stocks[0]?.limit_pct, 0.05)
})

test("local save repository rejects lossy daily trade statistics", () => {
  const code = Object.keys(mature.snapshot.daily_candles)[0]
  if (code === undefined) throw new Error("mature save must contain candles")
  const candles = mature.snapshot.daily_candles[code]
  const candle = candles?.[0]
  if (candle === undefined) throw new Error("mature save must contain a candle")
  const daily_candles = { ...mature.snapshot.daily_candles, [code]: [{ ...candle, trade_stats: { turnover_cents: 42, trade_count: 1 } }] }
  assert.throws(() => repositoryFor({ ...mature, snapshot: { ...mature.snapshot, daily_candles } }).load(), /turnover_cents/)
})
