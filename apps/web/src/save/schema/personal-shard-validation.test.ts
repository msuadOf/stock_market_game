import assert from "node:assert/strict"
import test from "node:test"
import { PersonalShardValidation, PERSONAL_SCHEMA_FIELDS, SHARDED_SCHEMA_COVERAGE, materializePersonalShard } from "./personal-shard-validation.ts"
import { scanArchiveRoot } from "./archive-root-scan.ts"
import { parseInformationStates } from "./personal/information.ts"
import { parseBeliefBooks } from "./personal/beliefs.ts"
import { parseRetailExperienceStates } from "./personal/experience.ts"
import { parseWatchlists, parsePriceMemories, parseHistoryReads } from "./personal/memory.ts"
import { parseNpcAttention } from "./npc-attention.ts"
import { map, safeIntegerKey, materializeSaveValue, RawSaveNumber } from "./primitives.ts"
import { parseSaveSlot } from "../save-schema.ts"
import { currentSaveFixture } from "../current-save-fixture.ts"
import { parseRawSaveJson } from "./raw-json.ts"

async function* chunks(text: string): AsyncIterable<string> { for (let offset = 0; offset < text.length; offset += 11) yield text.slice(offset, offset + 11) }
async function partial(value: unknown): Promise<ReturnType<PersonalShardValidation["finish"]>> {
  return partialText(JSON.stringify(value))
}
async function partialText(text: string): Promise<ReturnType<PersonalShardValidation["finish"]>> {
  const validator = new PersonalShardValidation()
  await scanArchiveRoot(chunks(text), validator.options())
  const summary = validator.finish()
  validator.ledger.throwIfAny()
  return summary
}
function failure(operation: () => unknown): string {
  try { operation() } catch (error) { assert.ok(error instanceof Error); return error.message }
  return "ACCEPTED"
}

const experience = { reference_equity: null, peak_equity: null, consecutive_failed_buys: 0, stocks: {} }
// synthetic：覆盖 parser 领域形状，不冒称 Rust producer 或 engine 恢复证据。
const book = { npc: "1", profile: { Retail: "Noise" }, analysis: { fundamental_bp: 0, trend_bp: 2500, price_volume_bp: 2500, technical_bp: 2500, experience_cost_bp: 2500, fundamental_method: null }, assumptions: { growth_deviation_bp: 0, quality_coefficient_bp: 0, pe_multiple: 0, equity_cost_bp: 0, terminal_growth_bp: 0, roe_deviation_bp: 0 }, entries: {}, experience, institution_policy: null, institution_account_risk_paused: false }
const attention = { base_probability: 0.1, next_attention_candidate_tick: "12", rng_state: "7", information_cadence: { Daily: { checks: 2 } }, next_information_check: { date: "2030-01-05", second_of_day: 43200 } }

test("覆盖台账恰好覆盖旧 root 全部34字段，codec覆盖仍不可签票", { timeout: 10000 }, async () => {
  const root = currentSaveFixture()
  assert.doesNotThrow(() => parseSaveSlot(root))
  assert.deepEqual(Object.keys(SHARDED_SCHEMA_COVERAGE).sort(), Object.keys(root).sort())
  assert.equal(Object.keys(SHARDED_SCHEMA_COVERAGE).length, 34)
  assert.equal(Object.values(SHARDED_SCHEMA_COVERAGE).filter((status: string) => status === "pending").length, 0)
  const summary = await partial({})
  assert.equal(summary.schemaValidated, false)
})

test("7张个人表逐行完整校验但只保留计数", { timeout: 10000 }, async () => {
  const input = { npc_attention: { "1": attention }, retail_experience: { "1": experience }, information_states: { "1": { owner: "1", companies: {} } }, belief_books: { "1": book }, watchlists: { "1": { stocks: {}, latest_attention_minute: "0" } }, price_memories: { "1": { stocks: {} } }, history_reads: { "1": { stocks: {} } } }
  const summary = await partial(input)
  assert.deepEqual(summary.counts, Object.fromEntries(PERSONAL_SCHEMA_FIELDS.map(field => [field, 1])))
  assert.equal(summary.schemaValidated, false)
})

test("个人表 scalar、未知字段及坏数字与旧整表 codec 同错", { timeout: 10000 }, async () => {
  const parsers: Record<string, (value: unknown) => unknown> = { npc_attention: value => map(value, "npc_attention", safeIntegerKey, parseNpcAttention), retail_experience: parseRetailExperienceStates, information_states: parseInformationStates, belief_books: parseBeliefBooks, watchlists: parseWatchlists, price_memories: parsePriceMemories, history_reads: parseHistoryReads }
  for (const [field, parse] of Object.entries(parsers)) {
    for (const value of [null, [], { "01": {} }, { "1": {} }]) {
      const expected = failure(() => parse(value))
      assert.notEqual(expected, "ACCEPTED")
      await assert.rejects(partial({ [field]: value }), error => error instanceof Error && error.message === expected)
    }
  }
})

test("owner post-check 必须晚于所有 leaf，整数键重排不改变首错", { timeout: 10000 }, async () => {
  for (const field of ["information_states", "belief_books"] as const) {
    const parse = field === "information_states" ? parseInformationStates : parseBeliefBooks
    const first = field === "information_states" ? { owner: "wrong", companies: {} } : { ...book, npc: "wrong" }
    const mismatch = field === "information_states" ? { owner: "999", companies: {} } : { ...book, npc: "999" }
    for (const rows of [{ "20": {}, "2": mismatch }, { "2": mismatch, "20": {} }, { "20": first, "2": {} }, { "2": {}, "20": first }]) {
      const expected = failure(() => parse(rows))
      await assert.rejects(partial({ [field]: rows }), error => error instanceof Error && error.message === expected)
    }
  }
})

test("原始 JSON 的物理键重排、非索引键和双重错误保持旧表首错", { timeout: 10000 }, async () => {
  for (const field of ["information_states", "belief_books"] as const) {
    const parse = field === "information_states" ? parseInformationStates : parseBeliefBooks
    const mismatch = field === "information_states" ? { owner: "999", companies: {} } : { ...book, npc: "999" }
    for (const entries of [
      '"20":{},"2":' + JSON.stringify(mismatch),
      '"2":' + JSON.stringify(mismatch) + ',"20":{}',
      '"01":{},"20":{},"2":' + JSON.stringify(mismatch),
      '"4294967295":{},"01":{}',
    ]) {
      const table = `{${entries}}`
      const expected = failure(() => parse(parseRawSaveJson(table)))
      await assert.rejects(partialText(`{"${field}":${table}}`), error => error instanceof Error && error.message === expected)
    }
  }
})

test("未消费 opaque 数值的分片物化路径与完整树相同", { timeout: 10000 }, () => {
  for (const field of PERSONAL_SCHEMA_FIELDS) {
    const row = { opaque: new RawSaveNumber("1") }
    const full = { [field]: { "2": row } }
    const parsed = field === "information_states" || field === "belief_books" ? row : full[field]
    const expected = failure(() => materializeSaveValue(full))
    assert.notEqual(expected, "ACCEPTED")
    assert.equal(failure(() => materializePersonalShard(field, "2", parsed)), expected)
  }
})
