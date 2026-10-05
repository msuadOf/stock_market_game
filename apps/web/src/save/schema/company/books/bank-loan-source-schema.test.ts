import assert from "node:assert/strict"
import test from "node:test"
import { parseBankBooks } from "./bank.ts"
import { bankLoanSourceFixture } from "./bank-loan-source-test-fixture.ts"

test("Bank 真实本金与利息来源严格往返，逾期计息不截断原期限", { timeout: 10000 }, () => {
  const saved = bankLoanSourceFixture()
  assert.deepEqual(parseBankBooks(saved, "Bank"), saved)
})

test("Bank 来源必填且不复制金额，不接受未知或重复实际来源", { timeout: 10000 }, () => {
  const saved = bankLoanSourceFixture()
  const { loan_claim_sources: _removed, ...missing } = saved
  assert.throws(() => parseBankBooks(missing, "Bank"), /loan_claim_sources/)
  for (const facts of [[], [saved.loan_claim_sources[0], saved.loan_claim_sources[0]], [{ ...saved.loan_claim_sources[0], amount: "80.00" }, saved.loan_claim_sources[1]], [{ ...saved.loan_claim_sources[0], loan: "toString" }, saved.loan_claim_sources[1]], [{ ...saved.loan_claim_sources[0], source: 9 }, saved.loan_claim_sources[1]], [{ ...saved.loan_claim_sources[0], kind: "Interest" }, saved.loan_claim_sources[1]]]) {
    assert.throws(() => parseBankBooks({ ...saved, loan_claim_sources: facts }, "Bank"), /loan_claim_sources/)
  }
})

test("Bank 来源核对真实凭证种类、日期、正额等额科目及完整覆盖", { timeout: 10000 }, () => {
  for (const mutate of [
    (saved: ReturnType<typeof bankLoanSourceFixture>) => { saved.books.journal.batches[0][0].date = "2030-01-02" },
    (saved: ReturnType<typeof bankLoanSourceFixture>) => { saved.books.journal.batches[0][1].cash_flow = "Operating" },
    (saved: ReturnType<typeof bankLoanSourceFixture>) => { saved.books.journal.batches[0][1].lines[0].account = "1301" },
    (saved: ReturnType<typeof bankLoanSourceFixture>) => { saved.books.journal.batches[0][1].lines[0].amount = "-1.00" },
    (saved: ReturnType<typeof bankLoanSourceFixture>) => { saved.books.journal.batches[0][1].lines[1].amount = "2.00" },
    (saved: ReturnType<typeof bankLoanSourceFixture>) => { saved.loan_claim_sources.pop() },
  ]) {
    const saved = bankLoanSourceFixture()
    mutate(saved)
    assert.throws(() => parseBankBooks(saved, "Bank"), /loan_claim_sources/)
  }
})

test("Bank 贷款原始日期与借款人必须明确有效", { timeout: 10000 }, () => {
  for (const change of [{ start_date: "2030-02-30" }, { maturity_date: "2030-01-01" }, { last_accrual_date: "2029-12-31" }, { counterparty: "toString" }, { rate_bp: -1 }]) {
    const saved = bankLoanSourceFixture()
    assert.throws(() => parseBankBooks({ ...saved, loans: { "LN-1": { ...saved.loans["LN-1"], ...change } } }, "Bank"))
  }
  for (const field of ["start_date", "maturity_date"] as const) {
    const saved = bankLoanSourceFixture()
    const loan: Record<string, unknown> = { ...saved.loans["LN-1"] }
    delete loan[field]
    assert.throws(() => parseBankBooks({ ...saved, loans: { "LN-1": loan } }, "Bank"), new RegExp(field))
  }
})

test("Bank 拒绝已占用全局事件游标，空贷款也不绕过真实凭证", { timeout: 10000 }, () => {
  const collision = bankLoanSourceFixture()
  collision.next_event_id = 2
  assert.throws(() => parseBankBooks(collision, "Bank"), /next_event_id/)
  const empty = { ...collision, loans: {}, loan_claim_sources: [], books: { ...collision.books, journal: { batches: [[{ ...collision.books.journal.batches[0][0], kind: "OpeningBalance" }]], closed: [] } }, next_event_id: 1 }
  assert.throws(() => parseBankBooks(empty, "Bank"), /next_event_id/)
  const sparse = { ...bankLoanSourceFixture(), next_event_id: 99 }
  assert.deepEqual(parseBankBooks(sparse, "Bank"), sparse)
})

test("Bank 拒绝重复Journal来源，不能Map覆盖", { timeout: 10000 }, () => {
  for (const kind of ["LoanIssued", "CashRevenue"]) {
    const saved = bankLoanSourceFixture()
    saved.books.journal.batches.push([{ ...saved.books.journal.batches[0][0], kind }])
    assert.throws(() => parseBankBooks(saved, "Bank"), /journal/)
  }
})
