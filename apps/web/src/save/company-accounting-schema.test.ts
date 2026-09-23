import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"
import { parseBooks, parseChartOfAccounts, parseCompanySpec, parsePeriodStates } from "./schema/company/accounting/index.ts"

const mature = JSON.parse(readFileSync("/home/baiyifan/.claude/tmp/opencode/task29-save.json", "utf8"))
const company = mature.company_operations.companies["C-000812"]

test("accounting parsers preserve a populated mature common company slice", () => {
  assert.deepEqual(parseCompanySpec(company.spec, "company.spec"), company.spec)
  assert.deepEqual(parseBooks(company.books.Industrial.books, "company.books"), company.books.Industrial.books)
  assert.deepEqual(parsePeriodStates({ closed: ["2030-01"] }, "period_states"), { closed: ["2030-01"] })
})

test("accounting parsers reject malformed chart, journal, and specification fields", () => {
  const books = structuredClone(company.books.Industrial.books)
  assert.throws(() => parseChartOfAccounts({ ...books.chart, accounts: { ...books.chart.accounts, "1002": { ...books.chart.accounts["1002"], is_cash: "true" } } }, "books.chart"), /books\.chart\.accounts\.1002\.is_cash/)
  assert.throws(() => parseBooks({ ...books, journal: { ...books.journal, batches: [[{ ...books.journal.batches[0][0], date: "2030-02-30" }]] } }, "books"), /books\.journal\.batches\[0\]\[0\]\.date/)
  assert.throws(() => parseBooks({ ...books, journal: { ...books.journal, batches: [[{ ...books.journal.batches[0][0], lines: undefined }]] } }, "books"), /books\.journal\.batches\[0\]\[0\]\.lines/)
  assert.throws(() => parseBooks({ ...books, journal: { ...books.journal, batches: [[{ ...books.journal.batches[0][0], lines: [{ ...books.journal.batches[0][0].lines[0], amount: "100.001" }] }]] } }, "books"), /books\.journal\.batches\[0\]\[0\]\.lines\[0\]\.amount/)
  assert.throws(() => parseBooks({ ...books, journal: { ...books.journal, batches: [{ entry: books.journal.batches[0][0] }] } }, "books"), /books\.journal\.batches\[0\]/)
  assert.throws(() => parseBooks({ ...books, journal: { ...books.journal, batches: [[{ ...books.journal.batches[0][0], source: 9_007_199_254_740_992 }]] } }, "books"), /books\.journal\.batches\[0\]\[0\]\.source/)
  assert.throws(() => parseCompanySpec({ ...company.spec, industry: " " }, "company.spec"), /company\.spec\.industry/)
  assert.throws(() => parseCompanySpec({ ...company.spec, listed_stock: 42 }, "company.spec"), /company\.spec\.listed_stock/)
  assert.throws(() => parseCompanySpec({ ...company.spec, unexpected: true }, "company.spec"), /company\.spec\.unexpected/)
  assert.throws(() => parsePeriodStates({ closed: ["2200-01"] }, "period_states"), /period_states\.closed\[0\]/)
})
