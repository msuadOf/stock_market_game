import assert from "node:assert/strict"
import test from "node:test"
import { scanArchiveRoot } from "./archive-root-scan.ts"
import { validateRootMetadata } from "./root-metadata-validation.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"
import { PersonalShardValidation, SHARDED_SCHEMA_COVERAGE } from "./personal-shard-validation.ts"
import { ROOT_KEYS } from "./root.ts"
import { parseSaveJson } from "../save-schema.ts"
import { sample } from "./company/state-test-fixture.ts"
import { currentSaveFixture } from "../current-save-fixture.ts"

async function* chunks(text: string): AsyncIterable<string> { for (let index = 0; index < text.length; index += 19) yield text.slice(index, index + 19) }
async function grouped(text: string): Promise<void> {
  const ledger = new SchemaFailureLedger()
  const personal = new PersonalShardValidation(ledger)
  const scan = await scanArchiveRoot(chunks(text), personal.options())
  personal.finish()
  assert.equal(validateRootMetadata(scan, ledger).schemaValidated, false)
  ledger.throwIfAny()
}
function errorOf(operation: () => unknown): string {
  try { operation() } catch (error) { assert.ok(error instanceof Error); return error.message }
  return "ACCEPTED"
}

test("覆盖台账与原 ROOT_KEYS 精确同键且仍有未覆盖门禁", { timeout: 10000 }, () => {
  assert.deepEqual(Object.keys(SHARDED_SCHEMA_COVERAGE).sort(), [...ROOT_KEYS].sort())
  assert.equal(Object.values(SHARDED_SCHEMA_COVERAGE).filter(value => value === "metadata").length, 18)
  assert.equal(Object.values(SHARDED_SCHEMA_COVERAGE).filter((value: string) => value === "pending").length, 0)
})

test("root 原形状、旧字段和未知字段优先级与旧 parser 一致", { timeout: 10000 }, async () => {
  const valid = currentSaveFixture()
  for (const value of [null, [], false, 1, {}, { ...valid, pending_npc: {}, extra: 1 }, { ...valid, extra: 1 }]) {
    const text = JSON.stringify(value)
    const expected = errorOf(() => parseSaveJson(text))
    assert.notEqual(expected, "ACCEPTED")
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
  }
})

test("6字段原 codec 与个人表跨阶段双错保持同一首错", { timeout: 10000 }, async () => {
  const cases = [
    { field: "setup", value: null }, { field: "seed", value: false }, { field: "civil_clock", value: null },
    { field: "market_memberships", value: null }, { field: "book_next_sequences", value: { "01": "1" } }, { field: "urgency_policy", value: {} },
  ]
  for (const { field, value } of cases) {
    const root = { ...currentSaveFixture(), [field]: value, information_states: { "2": {} } }
    const text = JSON.stringify(root)
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
  }
  const root = { ...currentSaveFixture(), setup: null, npc_attention: { "1": {} } }
  const text = JSON.stringify(root)
  await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
  await grouped(JSON.stringify(currentSaveFixture()))
})

test("订单尾 scalar 和 desk 标记沿用旧 codec 及 root 首错位置", { timeout: 10000 }, async () => {
  for (const [field, value] of [["rng_state", false], ["next_order_id", 0], ["desk_mirrored_resting", "bad"]] as const) {
    const root = { ...currentSaveFixture(), [field]: value, information_states: { "2": {} } }
    const text = JSON.stringify(root)
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
  }
})

test("其余小字段保留原 codec、公司选择和相对个人表首错", { timeout: 10000 }, async () => {
  for (const [field, value] of [
    ["price_history", null], ["market_minute_closes", { "0": [{}] }], ["npc_order_lifecycles", [{}]], ["pending_player", [{}]],
    ["ingress_receipt_cursors", null], ["company_state", null], ["report_correction_operations", { x: {} }], ["public_library", null], ["disclosures", null],
  ] as const) {
    const root = { ...currentSaveFixture(), [field]: value, information_states: { "2": {} } }
    const text = JSON.stringify(root)
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
  }
})

test("账户键集合和证券键集合跨域首错优先于 seed 或个人错误", { timeout: 10000 }, async () => {
  for (const root of [
    { ...currentSaveFixture(), seed: null, history_reads: { "2": {} } },
    { ...currentSaveFixture(), book_next_sequences: {}, information_states: { "2": {} } },
  ]) {
    const text = JSON.stringify(root)
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
  }
})

test("history_reads 非对象仍保留原跨域键检查的首错", { timeout: 10000 }, async () => {
  for (const value of [null, [], [1], "", "x", false, 1]) {
    const text = JSON.stringify({ ...currentSaveFixture(), history_reads: value, seed: null })
    const expected = errorOf(() => parseSaveJson(text))
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
  }
})


test("公开报告 source 必须来自 selected 公司权威且先于个人表错误", { timeout: 10000 }, async () => {
  const root = sample(2)
  const library = root.public_library as { reports: { source: string }[] }
  assert.ok(library.reports.length > 0)
  library.reports[0]!.source = "SimpleGenerated"
  root.information_states = { "2": {} }
  const text = JSON.stringify(root)
  const expected = errorOf(() => parseSaveJson(text))
  assert.match(expected, /public_library\.reports\[0\]\.source/)
  await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
})
