import assert from "node:assert/strict"
import test from "node:test"
import { captureGzipArchiveSource } from "./archive-source.ts"
import { gzipSaveCodec } from "./save-repository.ts"
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts"
import { parseSaveJson } from "./save-schema.ts"
import { validateDayEndArchive } from "./day-end-candidate.ts"
import { SAVE_MATERIALIZATION_KEYS } from "./schema/materialization-order.ts"
import { validateCapturedArchiveSchema } from "./archive-schema-validation.ts"

const fixture = () => withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")
async function validateText(text: string) { return validateCapturedArchiveSchema(captureGzipArchiveSource(`gzip:${await gzipSaveCodec.encode(text)}`)) }
function errorOf(operation: () => unknown): string {
  try { operation() } catch (error) { assert.ok(error instanceof Error); return error.message }
  return "ACCEPTED"
}

test("同源两遍覆盖34 root字段、跨域和日终后仅交付报告，不开放生产准入", { timeout: 10000 }, async () => {
  const text = JSON.stringify(fixture())
  const source = captureGzipArchiveSource(`gzip:${await gzipSaveCodec.encode(text)}`)
  const result = await validateCapturedArchiveSchema(source)
  assert.equal(result.source, source)
  assert.equal(result.utf8Bytes, Buffer.byteLength(text))
  assert.equal(result.coveredFields.length, 34)
  assert.deepEqual(SAVE_MATERIALIZATION_KEYS, Object.keys(parseSaveJson(text)))
  assert.equal(result.schemaValidated, true)
  assert.equal(result.dayEndValidated, true)
  assert.equal(result.productionAdmission, false)
})

test("每个root字段的缺失与scalar畸形及跨阶段双错和完整parser保持同首错", { timeout: 10000 }, async () => {
  for (const key of Object.keys(fixture())) {
    for (const root of [Object.fromEntries(Object.entries(fixture()).filter(([field]) => field !== key)), ...[null, false, 1, "x", []].map(value => ({ ...fixture(), [key]: value })), { ...fixture(), [key]: false, information_states: { "2": {} } }]) {
      const text = JSON.stringify(root)
      const expected = errorOf(() => validateDayEndArchive(parseSaveJson(text)))
      if (expected === "ACCEPTED") await validateText(text)
      else await assert.rejects(validateText(text), error => error instanceof Error && error.message === expected, key)
    }
  }
})

test("成员账户、runtime证券及cutoff全域检查早于个人表错误", { timeout: 10000 }, async () => {
  const valid = fixture()
  const runtime = valid.runtime_state as Record<string, unknown>
  for (const root of [
    { ...valid, market_memberships: { members: {} }, information_states: { "2": {} } },
    { ...valid, runtime_state: { ...runtime, book_public_cutoff_sequences: {} }, information_states: { "2": {} } },
    { ...valid, runtime_state: { ...runtime, book_public_cutoff_sequences: { "0": "1" } }, information_states: { "2": {} } },
  ]) {
    const text = JSON.stringify(root)
    await assert.rejects(validateText(text), error => error instanceof Error && error.message === errorOf(() => validateDayEndArchive(parseSaveJson(text))))
  }
})

test("完整schema后仍拒绝未日结、日内tick与活动资源", { timeout: 10000 }, async () => {
  const valid = fixture(), snapshot = valid.snapshot as Record<string, unknown>
  const order = { id: 1, side: "Buy", price: "1120", qty: 100, original_qty: 100, filled_qty: 0, filled_value: "0", owner: "0", seq: 0 }
  for (const root of [currentSaveFixture(), { ...valid, snapshot: { ...snapshot, tick: 1 } }, { ...valid, resting_orders: { "0": [order] } }, { ...valid, pending_player: [{ owner: "0", intent: { Cancel: { code: 0, id: 1 } }, account_ordinal: "0", stock_ordinal: "0" }] }]) {
    const text = JSON.stringify(root)
    await assert.rejects(validateText(text), error => error instanceof Error && error.message === errorOf(() => validateDayEndArchive(parseSaveJson(text))))
  }
})

test("晚词法与gzip错误仍优先于早schema错误，源身份不能伪造", { timeout: 10000 }, async () => {
  await assert.rejects(validateText('{"seed":false,"x":1,"x":2}'), /包含重复 JSON 键/)
  await assert.rejects(validateCapturedArchiveSchema(Object.freeze({ kind: "captured-gzip-source", encodedCharacters: 0, schemaValidated: false })), /不是本模块捕获的身份/)
})
