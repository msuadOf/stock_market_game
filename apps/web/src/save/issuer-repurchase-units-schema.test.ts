import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"
import { parseSessionCorporateActions } from "./schema/corporate-actions.ts"
import type { Context, SessionCorporateActions } from "./schema/corporate-actions.ts"

type Mutable<T> = T extends readonly (infer V)[] ? Mutable<V>[] : T extends object ? { -readonly [K in keyof T]: Mutable<T[K]> } : T
type Fixture = Mutable<{ actions: SessionCorporateActions; context: Context }>

function fixture(name: string): Fixture {
  return JSON.parse(readFileSync(new URL(`./fixtures/task174-${name}.json`, import.meta.url), "utf8")) as Fixture
}

for (const name of ["carry-two-consolidations", "equal-one-unit", "r-day-old-units", "zero-inventory-split-fill", "zero-inventory-consolidation-fill", "named-initial-plan", "backfilled-current-unit"]) {
  test(`真实 Rust codec 的回购单位事实通过严格 Web 解析：${name}`, () => {
    const value = fixture(name)
    assert.deepEqual(parseSessionCorporateActions(value.actions, value.context), value.actions)
  })
}

test("回填批准日期的当前锚方案不因旧事件缺单位事实被拒", () => {
  const value = fixture("backfilled-current-unit")
  const book = value.actions.issuer_repurchases.find(book => book.plan.event_id === "backfill-current-unit")!
  assert.equal(book.plan.approved_on, "2030-01-02")
  assert.equal(book.plan.approval_unit_event_id, "backfill-split1")
  assert.deepEqual(book.share_unit_adjustments, [])
})

test("空 R 库存也必须有 0→0 单位事实，不能把新成交重新当旧单位", () => {
  const value = fixture("zero-inventory-consolidation-fill")
  assert.deepEqual(value.actions.issuer_repurchases[0]!.share_unit_adjustments.map(item => [item.shares_before, item.shares_after]), [["0", "0"]])
  value.actions.issuer_repurchases[0]!.share_unit_adjustments = []
  assert.throws(() => parseSessionCorporateActions(value.actions, value.context), /单位事实/)
})

test("等量 1→1 并不表示股份单位未变化，缺事实必须拒绝", () => {
  const value = fixture("equal-one-unit")
  assert.deepEqual(value.actions.issuer_repurchases[0]!.share_unit_adjustments.map(item => [item.shares_before, item.shares_after]), [["1", "1"]])
  value.actions.issuer_repurchases[0]!.share_unit_adjustments = []
  assert.throws(() => parseSessionCorporateActions(value.actions, value.context), /单位事实/)
})

test("奖励与批准上限合并进位后允许残股 1，但不允许篡改实际冻结分桶奖励", () => {
  const value = fixture("carry-two-consolidations")
  const book = value.actions.issuer_repurchases.find(book => book.plan.event_id === "repurchase-2030")!
  assert.equal(book.status, "Cancelled")
  assert.deepEqual(book.share_unit_adjustments.map(item => [item.shares_before, item.shares_after]), [["99", "17"], ["17", "1"]])
  assert.doesNotThrow(() => parseSessionCorporateActions(value.actions, value.context))
  book.share_unit_adjustments.find(item => item.split_event_id === "carry-seventeen")!.shares_after = "2"
  assert.throws(() => parseSessionCorporateActions(value.actions, value.context), /冻结分桶实际奖励/)
})

test("单位调整错事件与重复身份都显式拒绝", () => {
  const original = fixture("carry-two-consolidations")
  const wrong = structuredClone(original)
  wrong.actions.issuer_repurchases[0]!.share_unit_adjustments[0]!.split_event_id = "unknown-event"
  assert.throws(() => parseSessionCorporateActions(wrong.actions, wrong.context), /未知|未登记/)
  const duplicate = structuredClone(original)
  duplicate.actions.issuer_repurchases[0]!.share_unit_adjustments.push(duplicate.actions.issuer_repurchases[0]!.share_unit_adjustments[0]!)
  assert.throws(() => parseSessionCorporateActions(duplicate.actions, duplicate.context), /不得重复/)
})

test("真实回购账户持仓必须等于各 plan 当前库存，不吸收 initial 残桶", () => {
  const value = fixture("zero-inventory-split-fill")
  const counts = value.context.setup.npcs!
  const accountId = String(counts.retail_count + counts.inst_count + counts.hot_count + 1)
  const stock = value.actions.issuer_repurchases[0]!.plan.stock
  value.context.snapshot.accounts[accountId]!.positions[stock]!.qty += 1
  assert.throws(() => parseSessionCorporateActions(value.actions, value.context), /账户实际持仓/)
})

test("公开市场不能用显式 lot 消耗绕过 FIFO 或限售", () => {
  const value = fixture("zero-inventory-split-fill")
  const receipt = value.actions.registries[0]!.receipts.find(receipt => receipt.request.scope === "PublicMarket" && receipt.request.changes.some(change => BigInt(change.change) < 0n))!
  const change = receipt.request.changes.find(change => BigInt(change.change) < 0n)!
  const disposal = receipt.disposals[0]!
  change.consume = [{ lot_id: disposal.lot.id, qty: disposal.lot.qty }]
  assert.throws(() => parseSessionCorporateActions(value.actions, value.context), /显式消耗选择只允许/)
})
