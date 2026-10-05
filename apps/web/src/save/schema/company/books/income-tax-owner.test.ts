import assert from "node:assert/strict"
import test from "node:test"
import { parseIndustryBooks } from "./index.ts"
import { accountingMinorUnits } from "../accounting/amount.ts"

import { policy, position, definitions, books, owners } from "./tax-owner-test-fixture.ts"

test("共享TaxOwner FIFO抵扣多个亏损年且拒绝i128基点中间乘积溢出", { timeout: 10000 }, () => {
  const annual = { loss_pool: [{ origin_year: 2028, remaining: "1.00" }], assessments: { "2030": { pretax: "6.00", current_tax: "0.00", opening_loss_pool: [{ origin_year: 2027, remaining: "3.00" }, { origin_year: 2028, remaining: "4.00" }], deferred_tax_asset: "0.25" } }, initial_deferred_tax_asset: "0.00", restatements: {} }
  const entry = { source: 1, date: "2030-12-31", kind: "TaxAccrual", cash_flow: "NonCash", lines: [{ account: "1811", side: "Debit", amount: "0.25" }, { account: "6801", side: "Credit", amount: "0.25" }] }
  const saved = { ...owners.Bank, income_tax_position: annual, books: { ...books, journal: { ...books.journal, batches: [[entry]] } }, next_event_id: 2 }
  assert.deepEqual(parseIndustryBooks({ Bank: saved }), { Bank: saved })
  assert.throws(() => parseIndustryBooks({ Bank: { ...saved, income_tax_position: { ...annual, loss_pool: [{ origin_year: 2027, remaining: "1.00" }] } } }), /亏损池/)
  const overflow = { ...position, assessments: { "2030": { pretax: "1701411834604692317316873037158841057.27", current_tax: "0.00", opening_loss_pool: [], deferred_tax_asset: "0.00" } } }
  assert.throws(() => parseIndustryBooks({ Bank: { ...owners.Bank, income_tax_position: overflow } }), /i128/)
  assert.throws(() => accountingMinorUnits("-1701411834604692317316873037158841057.28"), /Engine i128解码/)
  assert.equal(accountingMinorUnits("-1701411834604692317316873037158841057.27"), -((1n << 127n) - 1n))
})

test("共享TaxOwner拒绝tax科目cash/contra、伪计提与退款或非现金抵税", { timeout: 10000 }, () => {
  const annual = { ...position, assessments: { "2030": { pretax: "1.00", current_tax: "0.25", opening_loss_pool: [], deferred_tax_asset: "0.00" } } }
  const opening = { source: 0, date: "2030-01-01", kind: "OpeningBalance", cash_flow: "NonCash", lines: [{ account: "1001", side: "Debit", amount: "1.00" }, { account: "4001", side: "Credit", amount: "1.00" }] }
  const accrual = { source: 1, date: "2030-12-31", kind: "TaxAccrual", cash_flow: "NonCash", lines: [{ account: "6801", side: "Debit", amount: "0.25" }, { account: "222104", side: "Credit", amount: "0.25" }] }
  const payment = { source: 2, date: "2031-01-01", kind: "TaxPayment", cash_flow: "Operating", lines: [{ account: "222104", side: "Debit", amount: "0.25" }, { account: "1001", side: "Credit", amount: "0.25" }] }
  const chart = { version: 1, accounts: { ...definitions, "4001": { name: "权益", element: "Equity", is_cash: false, is_contra: false }, "1122": { name: "应收款", element: "Asset", is_cash: false, is_contra: false } } }
  const saved = { ...owners.Bank, income_tax_position: annual, books: { chart, journal: { batches: [[opening, accrual, payment]], closed: [] } }, next_event_id: 3 }
  assert.deepEqual(parseIndustryBooks({ Bank: saved }), { Bank: saved })
  const hidden = { ...owners.Bank, books: { chart, journal: { batches: [[opening, { ...accrual, kind: "CashRevenue" }]], closed: [] } }, next_event_id: 2 }
  assert.throws(() => parseIndustryBooks({ Bank: hidden }), /不能隐藏为普通经营业务/)
  for (const invalid of [{ ...accrual, cash_flow: "Operating" }, { ...accrual, lines: [...accrual.lines, { account: "1001", side: "Debit", amount: "0.01" }, { account: "1001", side: "Credit", amount: "0.01" }] }]) {
    assert.throws(() => parseIndustryBooks({ Bank: { ...saved, books: { chart, journal: { batches: [[opening, invalid, payment]], closed: [] } } } }), /纯税务NonCash/)
  }
  for (const invalid of [{ ...payment, cash_flow: "NonCash" }, { ...payment, lines: [{ account: "1001", side: "Debit", amount: "0.25" }, { account: "222104", side: "Credit", amount: "0.25" }] }, { ...payment, lines: [{ account: "222104", side: "Debit", amount: "0.25" }, { account: "1122", side: "Credit", amount: "0.25" }] }]) {
    assert.throws(() => parseIndustryBooks({ Bank: { ...saved, books: { chart, journal: { batches: [[opening, accrual, invalid]], closed: [] } } } }), /不能退款或非现金抵税/)
  }
  for (const code of ["1811", "222104", "6801"] as const) {
    const definition = definitions[code]
    for (const changes of [{ ...definition, is_contra: true }, { ...definition, is_cash: true }, { ...definition, element: definition.element === "Asset" ? "Liability" : "Asset" }]) {
      assert.throws(() => parseIndustryBooks({ Bank: { ...saved, books: { ...saved.books, chart: { ...chart, accounts: { ...chart.accounts, [code]: changes } } } } }), new RegExp(`科目${code}`))
    }
  }
  for (const restatements of [{ "9": "2030-01" }, { "2": "2031-01" }, { "2": "2031-02" }]) {
    assert.throws(() => parseIndustryBooks({ Bank: { ...saved, income_tax_position: { ...annual, restatements } } }), /restatements/)
  }
})

for (const [kind, owner] of Object.entries(owners)) {
  test(`${kind}共同税Owner按半偶落分且亏损仅在超结转年限后到期`, { timeout: 10000 }, () => {
    const first = { pretax: "0.02", current_tax: "0.00", opening_loss_pool: [], deferred_tax_asset: "0.00" }
    const second = { ...first, pretax: "0.06", current_tax: "0.02" }
    const chain = { ...position, assessments: { "2030": first, "2031": second } }
    const accrual = { source: 3, date: "2031-12-31", kind: "TaxAccrual", cash_flow: "NonCash", lines: [{ account: "6801", side: "Debit", amount: "0.02" }, { account: "222104", side: "Credit", amount: "0.02" }] }
    const saved = { ...owner, income_tax_position: chain, books: { ...books, journal: { ...books.journal, batches: [[accrual]] } }, next_event_id: 4 }
    assert.deepEqual(parseIndustryBooks({ [kind]: saved }), { [kind]: saved })
    assert.throws(() => parseIndustryBooks({ [kind]: { ...saved, income_tax_position: { ...chain, assessments: { "2030": { ...first, current_tax: "0.01" }, "2031": second } } } }), /半偶/)
    for (const origin of [2022, 2023]) {
      const tax = origin === 2022 ? "2.50" : "0.00"
      const annual = { ...position, assessments: { "2028": { pretax: "10.00", current_tax: tax, opening_loss_pool: [{ origin_year: origin, remaining: "10.00" }], deferred_tax_asset: "0.00" } } }
      const recorded = { ...accrual, date: "2028-12-31", lines: accrual.lines.map((line) => ({ ...line, amount: tax })) }
      const current = { ...owner, income_tax_position: annual, books: { ...books, journal: { ...books.journal, batches: origin === 2022 ? [[recorded]] : [] } }, next_event_id: 4 }
      assert.deepEqual(parseIndustryBooks({ [kind]: current }), { [kind]: current })
    }
  })
  test(`${kind}当前TaxOwner保留必填policy/position，缺失不能fallback`, { timeout: 10000 }, () => {
    assert.deepEqual(parseIndustryBooks({ [kind]: owner }), { [kind]: owner })
    const { income_tax_position: removed, ...missingPosition } = owner
    assert.ok(removed)
    assert.throws(() => parseIndustryBooks({ [kind]: missingPosition }), /income_tax_position/)
    if ("income_tax_policy" in owner) {
      const { income_tax_policy: removedPolicy, ...missingPolicy } = owner
      assert.ok(removedPolicy)
      assert.throws(() => parseIndustryBooks({ [kind]: missingPolicy }), /income_tax_policy/)
    } else {
      const { income_tax: removedPolicy, ...missingPolicy } = owner.tax_policy
      assert.ok(removedPolicy)
      assert.throws(() => parseIndustryBooks({ [kind]: { ...owner, tax_policy: missingPolicy } }), /income_tax/)
    }
  })
  test(`${kind}拒绝负Tax/DTA、非法policy与不完整年度链`, { timeout: 10000 }, () => {
    for (const invalid of [
      { ...position, initial_deferred_tax_asset: "-0.01" },
      { ...position, assessments: { "2030": { pretax: "1.00", current_tax: "-0.25", opening_loss_pool: [], deferred_tax_asset: "0.00" } } },
      { ...position, assessments: { "2030": { pretax: "1.00", current_tax: "0.24", opening_loss_pool: [], deferred_tax_asset: "0.00" } } },
      { ...position, loss_pool: [{ origin_year: 2030, remaining: "0.00" }] },
    ]) assert.throws(() => parseIndustryBooks({ [kind]: { ...owner, income_tax_position: invalid } }), /income_tax_position/)
    const invalidPolicy = { ...policy, loss_carryforward_years: 0 }
    assert.throws(() => parseIndustryBooks({ [kind]: "tax_policy" in owner ? { ...owner, tax_policy: { ...owner.tax_policy, income_tax: invalidPolicy } } : { ...owner, income_tax_policy: invalidPolicy } }), /income_tax/)
  })
  test(`${kind}共用连续年度亏损弥补、半偶税額及GL税凭证校验`, { timeout: 10000 }, () => {
    const first = { pretax: "-10.00", current_tax: "0.00", opening_loss_pool: [], deferred_tax_asset: "2.50" }
    const second = { pretax: "15.00", current_tax: "1.25", opening_loss_pool: [{ origin_year: 2030, remaining: "10.00" }], deferred_tax_asset: "0.00" }
    const chain = { ...position, assessments: { "2030": first, "2031": second } }
    const accrued = [{ source: 1, date: "2030-12-31", kind: "TaxAccrual", cash_flow: "NonCash", lines: [{ account: "1811", side: "Debit", amount: "2.50" }, { account: "6801", side: "Credit", amount: "2.50" }] }, { source: 2, date: "2031-12-31", kind: "TaxAccrual", cash_flow: "NonCash", lines: [{ account: "6801", side: "Debit", amount: "3.75" }, { account: "1811", side: "Credit", amount: "2.50" }, { account: "222104", side: "Credit", amount: "1.25" }] }]
    const saved = { ...owner, income_tax_position: chain, books: { ...books, journal: { ...books.journal, batches: [accrued] } }, next_event_id: 3 }
    assert.deepEqual(parseIndustryBooks({ [kind]: saved }), { [kind]: saved })
    for (const invalid of [
      { ...chain, assessments: { "2030": first, "2032": second } },
      { ...chain, assessments: { "2030": first, "2031": { ...second, opening_loss_pool: [] } } },
      { ...chain, loss_pool: [{ origin_year: 2030, remaining: "1.00" }] },
      { ...chain, assessments: {} },
    ]) assert.throws(() => parseIndustryBooks({ [kind]: { ...saved, income_tax_position: invalid } }), /income_tax_position/)
    assert.throws(() => parseIndustryBooks({ [kind]: { ...saved, books } }), /income_tax_position/)
  })
}
