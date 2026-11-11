import assert from "node:assert/strict"
import test from "node:test"
import { currentSaveFixture } from "../current-save-fixture.ts"
import { parseSaveJson } from "../save-schema.ts"
import { scanArchiveRoot } from "./archive-root-scan.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"
import { PlanShardValidation } from "./plan-shard-validation.ts"

const company = { plan_id: { owner: "17", sequence: "0" }, issuer: 3, stock: 7, status: "Funded", execution: null }
const policy = { reverse_revision_threshold_bp: 2000, review_signal_delta_bp: 1000, review_price_change_bp: 200 }
const book = { policy, owner_sequences: [{ owner: "17", next_sequence: "1" }], plans: [{ IssuerRepurchase: company }] }
async function* chunks(text: string): AsyncIterable<string> { for (let index = 0; index < text.length; index += 13) yield text.slice(index, index + 13) }
async function grouped(text: string) {
  const ledger = new SchemaFailureLedger(), plans = new PlanShardValidation(ledger)
  const scan = await scanArchiveRoot(chunks(text), plans.options())
  const summary = plans.finish(scan)
  ledger.throwIfAny()
  return summary
}
function errorOf(operation: () => unknown): string {
  try { operation() } catch (error) { assert.ok(error instanceof Error); return error.message }
  return "ACCEPTED"
}

test("逐计划复用原codec并仅保留PlanId/股票/终止摘要", { timeout: 10000 }, async () => {
  const text = JSON.stringify({ ...currentSaveFixture(), plans: book })
  assert.doesNotThrow(() => parseSaveJson(text))
  const result = await grouped(text)
  assert.equal(result.plans, 1)
  assert.equal(result.hasIntradayExecution, false)
  assert.equal(result.schemaValidated, false)
})

test("plans根形状、policy、owner及plan leaf保持原首错", { timeout: 10000 }, async () => {
  for (const value of [null, [], false, 1, {}, { ...book, extra: true }, { ...book, policy: {} }, { ...book, owner_sequences: null }, { ...book, owner_sequences: [{}] }, { ...book, plans: null }, { ...book, plans: [{}] }, { ...book, plans: [{ Unknown: company }] }]) {
    const text = JSON.stringify({ ...currentSaveFixture(), plans: value })
    const expected = errorOf(() => parseSaveJson(text))
    assert.notEqual(expected, "ACCEPTED")
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
  }
})

test("所有plan leaf先于重复owner和PlanId/owner游标/active股票全书检查", { timeout: 10000 }, async () => {
  for (const value of [
    { ...book, owner_sequences: [book.owner_sequences[0], book.owner_sequences[0]], plans: [{}] },
    { ...book, owner_sequences: [], plans: [{ IssuerRepurchase: company }, {}] },
    { ...book, plans: [book.plans[0], book.plans[0], {}] },
    { ...book, owner_sequences: [book.owner_sequences[0], book.owner_sequences[0]] },
    { ...book, owner_sequences: [] },
    { ...book, owner_sequences: [{ owner: "17", next_sequence: "0" }] },
    { ...book, plans: [book.plans[0], book.plans[0]] },
    { ...book, owner_sequences: [{ owner: "17", next_sequence: "2" }], plans: [book.plans[0], { IssuerRepurchase: { ...company, plan_id: { owner: "17", sequence: "1" } } }] },
  ]) {
    const text = JSON.stringify({ ...currentSaveFixture(), plans: value })
    const expected = errorOf(() => parseSaveJson(text))
    assert.notEqual(expected, "ACCEPTED")
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
  }
})

test("计划执行腿被明确记入日终摘要，终止计划可复用股票", { timeout: 10000 }, async () => {
  for (const execution of [null, { child_qty: 100, limit_price: "1234", expires_market_minute: 30, active_child_remaining_qty: null }]) {
    const text = JSON.stringify({ ...currentSaveFixture(), plans: { ...book, plans: [{ IssuerRepurchase: { ...company, execution } }] } })
    const result = await grouped(text)
    assert.equal(result.hasIntradayExecution, execution !== null)
  }
  const text = JSON.stringify({ ...currentSaveFixture(), plans: { ...book, owner_sequences: [{ owner: "17", next_sequence: null }], plans: [book.plans[0], { IssuerRepurchase: { ...company, plan_id: { owner: "17", sequence: "18446744073709551615" }, status: "Closed" } }] } })
  await grouped(text)
})
