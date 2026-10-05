import assert from "node:assert/strict"
import test from "node:test"
import { parseReportCorrections, validateReportCorrectionLinks } from "./report-corrections.ts"
import type { parseCompanySystemState } from "./system.ts"
import type { PublicLibrary } from "./reports.ts"

const entry = { source: 7, date: "2030-01-02", kind: "CashRevenue", cash_flow: "NonCash", lines: [{ account: "1001", side: "Debit", amount: "10.00" }, { account: "6001", side: "Credit", amount: "10.00" }] }
const completed = { request: { operation_id: "correct-7", company: "C-600000", supersedes: 2, entries: [entry], reason: "更正真实凭证" }, publications: [3], committed_at: { date: "2030-01-02", second_of_day: 80000 } }

function fixture() {
  const system = { implementation: { mode: "Simple", state: { companies: { "C-600000": { finance: { company: "C-600000", books: { journal: { batches: [[entry]] } }, closing: { restatements: [[{ Standalone: "C-600000" }, [["7", "2030-01"]]]] } } } } } } } as unknown as ReturnType<typeof parseCompanySystemState>
  const original = { source: "SimpleGenerated", id: 2, company: "C-600000", published_at: { date: "2030-01-02", second_of_day: 1000 }, reports: { period: "2030-01", scope: { Standalone: "C-600000" } }, origin: "Scheduled", supersedes: null }
  const correction = { ...original, id: 3, published_at: completed.committed_at, origin: "Correction", supersedes: 2 }
  return { system, original, correction, library: { reports: [original, correction] } as unknown as PublicLibrary }
}

test("已完成report correction严格保留JournalEntry的会计元字符串与真实公开ID", { timeout: 10000 }, () => {
  assert.deepEqual(parseReportCorrections({ "correct-7": completed }), { "correct-7": completed })
  assert.deepEqual(parseReportCorrections({}), {})
  assert.throws(() => parseReportCorrections(undefined), /report_correction_operations/)
})

test("更正存档拒绝缺字段、无结果、operation身份漂移和私加审批时间", { timeout: 10000 }, () => {
  assert.throws(() => parseReportCorrections({ other: completed }), /operation/)
  assert.throws(() => parseReportCorrections({ "correct-7": { ...completed, publications: [] } }), /publications/)
  assert.throws(() => parseReportCorrections({ "correct-7": { ...completed, request: { ...completed.request, approved_at: completed.committed_at } } }), /approved_at/)
  assert.throws(() => parseReportCorrections({ "correct-7": { ...completed, request: { ...completed.request, reason: " " } } }), /reason/)
  assert.throws(() => parseReportCorrections({ "correct-7": { ...completed, publications: [3, 3] } }), /publications/)
})

test("更正凭证复用现行JournalEntry并拒绝不平衡和非正会计金额", { timeout: 10000 }, () => {
  for (const lines of [[], [{ ...entry.lines[0], amount: "0.00" }], [{ ...entry.lines[0], amount: "10.00" }, { ...entry.lines[1], amount: "9.99" }]]) {
    assert.throws(() => parseReportCorrections({ "correct-7": { ...completed, request: { ...completed.request, entries: [{ ...entry, lines }] } } }), /entries/)
  }
  assert.throws(() => parseReportCorrections({ "correct-7": { ...completed, request: { ...completed.request, supersedes: "2" } } }), /supersedes/)
})

test("更正完成事实必须关联真实owner凭证、有效期间及日终公开链", { timeout: 10000 }, () => {
  const parsed = parseReportCorrections({ "correct-7": completed })
  const { system, original, correction, library } = fixture()
  assert.doesNotThrow(() => validateReportCorrectionLinks(parsed, system, library, "2030-01-03"))
  const wrongSource = parseReportCorrections({ "correct-7": { ...completed, request: { ...completed.request, entries: [{ ...entry, source: 8 }] } } })
  assert.throws(() => validateReportCorrectionLinks(wrongSource, system, library, "2030-01-03"), /实际owner凭证/)
  assert.throws(() => validateReportCorrectionLinks(parsed, system, library, "2030-01-02"), /已结束/)
  assert.throws(() => validateReportCorrectionLinks(parsed, { implementation: { mode: "Simple", state: { companies: {} } } } as unknown as ReturnType<typeof parseCompanySystemState>, library, "2030-01-03"), /公司/)
  const unrelated = { reports: [original, { ...correction, company: "C-other", reports: { period: "2030-01", scope: { Consolidated: "C-other" } } }] } as unknown as PublicLibrary
  assert.throws(() => validateReportCorrectionLinks(parsed, system, unrelated, "2030-01-03"), /同一公司/)
  const changed = parseReportCorrections({ "correct-7": { ...completed, request: { ...completed.request, entries: [{ ...entry, source: 8 }] } } })
  assert.throws(() => validateReportCorrectionLinks(changed, system, library, "2030-01-03"), /实际owner凭证/)
  const alteredPeriod = { reports: [original, { ...correction, reports: { ...correction.reports, period: "2030-02" } }] } as unknown as PublicLibrary
  assert.throws(() => validateReportCorrectionLinks(parsed, system, alteredPeriod, "2030-01-03"), /替代原版/)
  const futureOriginal = { reports: [{ ...original, published_at: { date: "2030-01-03", second_of_day: 0 } }, correction] } as unknown as PublicLibrary
  assert.throws(() => validateReportCorrectionLinks(parsed, system, futureOriginal, "2030-01-03"), /尚未公开/)
  for (const text of ["10", "10.0", "+10.00", " 10.00 "]) {
    const equivalent = parseReportCorrections({ "correct-7": { ...completed, request: { ...completed.request, entries: [{ ...entry, lines: entry.lines.map((line) => ({ ...line, amount: text })) }] } } })
    assert.doesNotThrow(() => validateReportCorrectionLinks(equivalent, system, library, "2030-01-03"))
    assert.equal(equivalent["correct-7"].request.entries[0].lines[0].amount, text)
  }
  const different = parseReportCorrections({ "correct-7": { ...completed, request: { ...completed.request, entries: [{ ...entry, lines: entry.lines.map((line) => ({ ...line, amount: "11.00" })) }] } } })
  assert.throws(() => validateReportCorrectionLinks(different, system, library, "2030-01-03"), /实际owner凭证/)
  const cascadeInstant = { date: "2030-03-02", second_of_day: 80000 }
  const cascade = parseReportCorrections({ "correct-7": { ...completed, committed_at: cascadeInstant, publications: [3, 5] } })
  const cascadeLibrary = { reports: [original, { ...correction, published_at: cascadeInstant }, { ...original, id: 4, published_at: { date: "2030-02-02", second_of_day: 80000 }, reports: { ...original.reports, period: "2030-02" } }, { ...correction, id: 5, supersedes: 4, published_at: cascadeInstant, reports: { ...correction.reports, period: "2030-02" } }] } as unknown as PublicLibrary
  assert.doesNotThrow(() => validateReportCorrectionLinks(cascade, system, cascadeLibrary, "2030-03-03"))
  const consolidatedTarget = { reports: [{ ...original, reports: { ...original.reports, scope: { Consolidated: "C-600000" } } }, { ...correction, reports: { ...correction.reports, scope: { Consolidated: "C-600000" } } }] } as unknown as PublicLibrary
  assert.throws(() => validateReportCorrectionLinks(parsed, system, consolidatedTarget, "2030-01-03"), /原Standalone/)
})

test("更正operation身份不把合法对象键当prototype或静默丢失", { timeout: 10000 }, () => {
  const request = { ...completed.request, operation_id: "__proto__" }
  const source = Object.fromEntries([["__proto__", { ...completed, request }]])
  const parsed = parseReportCorrections(source)
  assert.equal(Object.hasOwn(parsed, "__proto__"), true)
  assert.equal(parsed["__proto__"].request.operation_id, "__proto__")
})

test("Simple 更正凭证必须完整匹配，不能仅复用现金或事件source", { timeout: 10000 }, () => {
  const { system, library } = fixture()
  for (const changes of [{ source: 8 }, { date: "2030-01-01" }, { kind: "CashExpense" }, { cash_flow: "Operating" }]) {
    const parsed = parseReportCorrections({ "correct-7": { ...completed, request: { ...completed.request, entries: [{ ...entry, ...changes }] } } })
    assert.throws(() => validateReportCorrectionLinks(parsed, system, library, "2030-01-03"), /实际owner凭证/)
  }
  const parsed = parseReportCorrections({ "correct-7": completed })
  const finance = system.implementation.state.companies["C-600000"]!.finance
  const duplicateJournal = structuredClone(system)
  const duplicateFinance = duplicateJournal.implementation.state.companies["C-600000"]!.finance
  Object.assign(duplicateFinance.books.journal, { batches: [...finance.books.journal.batches, [...finance.books.journal.batches[0]!]] })
  assert.throws(() => validateReportCorrectionLinks(parsed, duplicateJournal, library, "2030-01-03"), /重复/)
})

test("更正公开游标及closing来源映射不得重复或改为其他来源", { timeout: 10000 }, () => {
  const { system, library, correction } = fixture()
  const parsed = parseReportCorrections({ "correct-7": completed })
  const duplicate = { "correct-7": parsed["correct-7"]!, "correct-8": { ...parsed["correct-7"]!, request: { ...parsed["correct-7"]!.request, operation_id: "correct-8" } } }
  assert.throws(() => validateReportCorrectionLinks(duplicate, system, library, "2030-01-03"), /重复/)
  assert.throws(() => validateReportCorrectionLinks(parsed, system, { reports: [...library.reports, correction] } as unknown as PublicLibrary, "2030-01-03"), /重复/)
  const wrongSource = { reports: library.reports.map(report => ({ ...report, source: "SimulationAccounting" })) } as unknown as PublicLibrary
  assert.throws(() => validateReportCorrectionLinks(parsed, system, wrongSource, "2030-01-03"), /Simple/)
  const wrongOriginal = { reports: library.reports.map(report => report.id === 2 ? { ...report, source: "SimulationAccounting" } : report) } as unknown as PublicLibrary
  assert.throws(() => validateReportCorrectionLinks(parsed, system, wrongOriginal, "2030-01-03"), /Simple/)
  for (const mappings of [[], [[{ Standalone: "C-other" }, [["7", "2030-01"]]]], [[{ Standalone: "C-600000" }, [["7", "2030-02"]]]], [[{ Standalone: "C-600000" }, [["7", "2030-01"], ["7", "2030-01"]]]]]) {
    const altered = structuredClone(system)
    Object.assign(altered.implementation.state.companies["C-600000"]!.finance.closing, { restatements: mappings })
    assert.throws(() => validateReportCorrectionLinks(parsed, altered, library, "2030-01-03"), /有效期间/)
  }
})
