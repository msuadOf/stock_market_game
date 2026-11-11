import assert from "node:assert/strict"
import test from "node:test"
import { currentSaveFixture } from "../current-save-fixture.ts"
import { parseSaveJson } from "../save-schema.ts"
import { scanArchiveRoot } from "./archive-root-scan.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"
import { ReceiptShardValidation } from "./receipt-shard-validation.ts"

const row = { receipt_id: "1", civil_date: "2030-01-01", stock: 0, side: "Buy", price_cents: "1120", quantity_shares: 100, gross_cents: "112000", actual_fees: { commission: "500", stamp_tax: "0", transfer_fee: "1" } }
const context = { accounts: ["0", "1"], stocks: ["0"], startDate: "2030-01-01", currentDate: "2030-01-01" }
async function* chunks(text: string): AsyncIterable<string> { for (let index = 0; index < text.length; index += 17) yield text.slice(index, index + 17) }
async function grouped(text: string) {
  const ledger = new SchemaFailureLedger(), receipts = new ReceiptShardValidation(ledger, context)
  const scan = await scanArchiveRoot(chunks(text), receipts.options())
  const summary = receipts.finish(scan)
  ledger.throwIfAny()
  return summary
}
function errorOf(operation: () => unknown): string {
  try { operation() } catch (error) { assert.ok(error instanceof Error); return error.message }
  return "ACCEPTED"
}

test("逐receipt丢弃原行，只保留计数、每账户尾号和全局最大id", { timeout: 10000 }, async () => {
  const text = JSON.stringify({ ...currentSaveFixture(), personal_trade_confirmations: { "0": [row, { ...row, receipt_id: "3" }] }, receipt_watermark: 4 })
  assert.doesNotThrow(() => parseSaveJson(text))
  const result = await grouped(text)
  assert.equal(result.rows, 2)
  assert.equal(result.maximumReceiptId, "3")
  assert.equal(result.schemaValidated, false)
})

test("receipt root/account 形状、leaf、单调和watermark首错与原codec一致", { timeout: 10000 }, async () => {
  for (const value of [null, [], false, 1, { bad: [] }, { "0": null }, { "0": 1 }, { "0": [{}] }, { "0": [{ ...row, stock: -1 }] }, { "0": [{ ...row, gross_cents: "0" }] }, { "0": [row, row] }]) {
    const text = JSON.stringify({ ...currentSaveFixture(), personal_trade_confirmations: value, receipt_watermark: 0 })
    const expected = errorOf(() => parseSaveJson(text))
    assert.notEqual(expected, "ACCEPTED")
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
  }
})

test("某账户全部leaf早于本账户单调检查；本账户单调早于后账户leaf", { timeout: 10000 }, async () => {
  for (const value of [{ "0": [row, row, {}] }, { "0": [row, row], "1": [{}] }]) {
    const text = JSON.stringify({ ...currentSaveFixture(), personal_trade_confirmations: value, receipt_watermark: 0 })
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
  }
})

test("receipt owner/stock/date跨域检查先于watermark，空账户日志也检查归属", { timeout: 10000 }, async () => {
  for (const value of [{ "2": [] }, { "0": [{ ...row, stock: 1 }] }, { "0": [{ ...row, civil_date: "2029-12-31" }] }, { "0": [{ ...row, civil_date: "2030-01-02" }] }, { "0": [row] }]) {
    const text = JSON.stringify({ ...currentSaveFixture(), personal_trade_confirmations: value, receipt_watermark: 0 })
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
  }
})

test("receipt物理账户顺序不影响Object.keys首错，非索引数字键保留插入序", { timeout: 10000 }, async () => {
  const validText = JSON.stringify(currentSaveFixture())
  for (const payload of [
    '{"20":[{}],"2":[{"receipt_id":"1"}]}',
    `{"00":[${JSON.stringify(row)},${JSON.stringify(row)}],"4294967295":[{}]}`,
    `{"4294967295":[{}],"00":[${JSON.stringify(row)},${JSON.stringify(row)}]}`,
  ]) {
    const text = validText.replace('"personal_trade_confirmations":{}', `"personal_trade_confirmations":${payload}`)
    const expected = errorOf(() => parseSaveJson(text))
    await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
  }
})

test("watermark边界与超safe-id仍用原精确整数字义", { timeout: 10000 }, async () => {
  for (const value of [false, -1, 1.5, "3", 3, 4]) {
    const text = JSON.stringify({ ...currentSaveFixture(), personal_trade_confirmations: { "0": [row, { ...row, receipt_id: "3" }] }, receipt_watermark: value })
    const expected = errorOf(() => parseSaveJson(text))
    if (expected === "ACCEPTED") await grouped(text)
    else await assert.rejects(grouped(text), error => error instanceof Error && error.message === expected)
  }
  const text = JSON.stringify({ ...currentSaveFixture(), personal_trade_confirmations: { "0": [{ ...row, receipt_id: "9007199254740993" }] }, receipt_watermark: 9007199254740991 })
  await assert.rejects(grouped(text), error => error instanceof Error && error.message === errorOf(() => parseSaveJson(text)))
})
