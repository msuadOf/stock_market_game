import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"
import { parseSessionCorporateActions } from "./schema/corporate-actions.ts"
import type { Context, SessionCorporateActions } from "./schema/corporate-actions.ts"

type Mutable<T> = T extends readonly (infer V)[] ? Mutable<V>[] : T extends object ? { -readonly [K in keyof T]: Mutable<T[K]> } : T
type Fixture = Mutable<{ actions: SessionCorporateActions; context: Context }>

function fixture(name: string): Fixture {
  return JSON.parse(readFileSync(new URL(`./fixtures/task175-${name}.json`, import.meta.url), "utf8")) as Fixture
}

// 送股个税（任务175）：真实 Rust codec 输出的送股税基与处置扣缴事实必须通过
// 严格 Web 解析且深等价；转增情景不得携带任何税基事实。
for (const name of ["bonus-sale-withholding", "conversion-no-basis"]) {
  test(`真实 Rust codec 的送转个税事实通过严格 Web 解析：${name}`, () => {
    const value = fixture(name)
    assert.deepEqual(parseSessionCorporateActions(value.actions, value.context), value.actions)
  })
}

test("送股税基事实携带面值与数量，且满月当日卖出扣缴 20%", () => {
  const value = fixture("bonus-sale-withholding")
  const taxBook = value.actions.dividend_tax_books.find(book => book.account === "0")!
  assert.equal(taxBook.stock_bonus_bases.length, 1)
  const basis = taxBook.stock_bonus_bases[0]!
  assert.equal(basis.event_id, "distribution-1")
  assert.equal(basis.credited_on, "2030-01-07")
  assert.equal(basis.lot_id, "tax:stock-distribution:distribution-1:account-0")
  assert.equal(basis.qty, "3")
  assert.equal(basis.par_value_per_share, "100")
  const collection = taxBook.collections.at(-1)!
  assert.equal(collection.collected, "60", "面值 100 分 × 3 股 × 20%（满月当日）")
})

test("转增情景不携带任何送股税基事实", () => {
  const value = fixture("conversion-no-basis")
  for (const book of value.actions.dividend_tax_books) {
    assert.deepEqual(book.stock_bonus_bases, [])
  }
  assert.equal(value.actions.stock_distributions[0]!.plan.kind, "CapitalReserveConversion")
})

test("缺 stock_bonus_bases 键、未知键与篡改数量都被显式拒绝", () => {
  const original = fixture("bonus-sale-withholding")
  const missing = structuredClone(original)
  delete (missing.actions.dividend_tax_books[0] as { stock_bonus_bases?: unknown }).stock_bonus_bases
  assert.throws(() => parseSessionCorporateActions(missing.actions, missing.context))
  const unknown = structuredClone(original)
  ;(unknown.actions.dividend_tax_books[0] as { stock_bonus_bases: unknown[] }).stock_bonus_bases.push({ operation_seq: 99 })
  assert.throws(() => parseSessionCorporateActions(unknown.actions, unknown.context))
  const tampered = structuredClone(original)
  tampered.actions.dividend_tax_books[0]!.stock_bonus_bases[0]!.qty = "2"
  assert.throws(() => parseSessionCorporateActions(tampered.actions, tampered.context), /送股税基与入账税批次事实不一致|数量与名册非交易过户分录不一致/)
  const duplicated = structuredClone(original)
  const cloned = { ...duplicated.actions.dividend_tax_books[0]!.stock_bonus_bases[0]! }
  cloned.operation_seq = duplicated.actions.dividend_tax_books[0]!.operation_seq + 1
  duplicated.actions.dividend_tax_books[0]!.stock_bonus_bases.push(cloned)
  assert.throws(() => parseSessionCorporateActions(duplicated.actions, duplicated.context), /税账操作序列|同一税批次不得重复/)
})

test("税基事件缺已入账送股账簿与清空税基都被勾稽拒绝", () => {
  const original = fixture("bonus-sale-withholding")
  const ghost = structuredClone(original)
  ghost.actions.dividend_tax_books[0]!.stock_bonus_bases[0]!.event_id = "ghost-event"
  // 账本级回放先拒（事件归属与税批次来源不一致），不进入会话勾稽也是显式拒绝。
  assert.throws(() => parseSessionCorporateActions(ghost.actions, ghost.context), /送股税基|缺少已入账的送股/)
  const emptied = structuredClone(original)
  emptied.actions.dividend_tax_books[0]!.stock_bonus_bases = []
  // 账本级操作序先拒（事实数与 operation_seq 不一致），显式失败而非静默通过。
  assert.throws(() => parseSessionCorporateActions(emptied.actions, emptied.context))
})

test("转增事件不得携带送股税基", () => {
  const original = fixture("conversion-no-basis")
  const forged = structuredClone(original)
  const taxBook = forged.actions.dividend_tax_books.find(book => book.account === "0")!
  // 从 bonus fixture 移植一条税基到转增情景：事件指向已入账的转增事件。
  const bonus = fixture("bonus-sale-withholding")
  const basis = structuredClone(bonus.actions.dividend_tax_books.find(book => book.account === "0")!.stock_bonus_bases[0]!)
  basis.event_id = forged.actions.stock_distributions[0]!.plan.event_id
  basis.credited_on = forged.actions.stock_distributions[0]!.credited_on!
  basis.operation_seq = taxBook.operation_seq + 1
  basis.lot_id = `tax:stock-distribution:${basis.event_id}:account-0`
  taxBook.stock_bonus_bases.push(basis)
  taxBook.operation_seq += 1
  // 该转增批次税账日结存在（税基引用的 lot 也存在），但转增免税：勾稽必须拒绝。
  assert.throws(() => parseSessionCorporateActions(forged.actions, forged.context), /BonusShares|送股/)
})
