import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import fs from "node:fs"
import { syncBuiltinESMExports } from "node:module"
import test from "node:test"
import { currentSaveFixture } from "./current-save-fixture.ts"
import { parseCompanySpec } from "./schema/company/accounting/spec.ts"
import { parseCompanyOperations } from "./schema/company/operations.ts"
import { parsePublicLibrary, parseReportSet } from "./schema/company/reports.ts"
import { representativeCompanyOperationsFixture } from "./company-slice-test-fixture.ts"
import { parseIndustrialBooks } from "./schema/company/books/industrial.ts"
import { parseClosingRegistry } from "./schema/company/closing.ts"

const fragments = JSON.parse(readFileSync(new URL("./fixtures/current-producer-fragments.json", import.meta.url), "utf8")) as Record<string, unknown>

test("真实 CompanySpec 使用 code 和完整 u64 issued_shares", { timeout: 10_000 }, () => {
  assert.deepEqual(parseCompanySpec(fragments.company_spec, "spec"), fragments.company_spec)
  const spec = fragments.company_spec as Record<string, unknown>
  assert.equal(parseCompanySpec({ ...spec, issued_shares: "18446744073709551615" }, "spec").issued_shares, "18446744073709551615")
  for (const invalid of [0, "0", "01", "+1", "-1", "18446744073709551616"]) assert.throws(() => parseCompanySpec({ ...spec, issued_shares: invalid }, "spec"), /issued_shares/)
  const { code: _removed, ...missing } = spec
  assert.throws(() => parseCompanySpec({ ...missing, id: "C-600101" }, "spec"), /code/)
  assert.throws(() => parseCompanySpec({ ...spec, extra: true }, "spec"), /extra/)
})

test("CompanyOperations 保留严格 catalog，拒绝重复代码、缺字段与派生索引", { timeout: 10_000 }, () => {
  const saved = { ...((currentSaveFixture().company_state as { LegacyOperations: { operations: Record<string, unknown> } }).LegacyOperations.operations), catalog: fragments.catalog }
  assert.deepEqual(parseCompanyOperations(saved), saved)
  const { catalog: _removed, ...missing } = saved
  assert.throws(() => parseCompanyOperations(missing), /catalog/)
  for (const catalog of [{}, { metas: [{ code: "C", name: "公司", extra: true }] }, { metas: [{ code: "C", name: "公司" }, { code: "C", name: "另一公司" }] }, { metas: [], by_code: {} }]) {
    assert.throws(() => parseCompanyOperations({ ...saved, catalog }), /catalog/)
  }
})

test("真实报告保留来源和数值 CompanyId/MemberId，并拒绝旧字符串或越界身份", { timeout: 10_000 }, () => {
  const report = fragments.report as Record<string, unknown>
  const library = { next_seq: 1, reports: [report], announcements: [] }
  assert.deepEqual(parsePublicLibrary(library), library)
  const reports = report.reports as Record<string, unknown>
  assert.deepEqual(parseReportSet(reports, "reports"), reports)
  const closing = { versions: [[reports.scope, reports.period, reports.kind, [reports]]], restatements: [] }
  assert.deepEqual(parseClosingRegistry(closing), closing)
  for (const source of ["Unknown", null, undefined]) assert.throws(() => parsePublicLibrary({ ...library, reports: [{ ...report, source }] }), /source/)
  for (const company of ["0", -1, 4_294_967_296]) {
    assert.throws(() => parsePublicLibrary({ ...library, reports: [{ ...report, company }] }), /company/)
    assert.throws(() => parseReportSet({ ...reports, scope: { Standalone: company } }, "reports"), /scope/)
  }
})


test("真实所得税仓位保留年度 assessments 链、初始递延资产与 restatements", { timeout: 10_000 }, () => {
  const operations = representativeCompanyOperationsFixture()
  const company = Object.values(operations.companies as Record<string, { books: { Industrial: Record<string, unknown> } }>)[0]!
  const books = { ...company.books.Industrial, income_tax_position: fragments.income_tax_position }
  assert.deepEqual(parseIndustrialBooks(books, "books"), books)
  const position = fragments.income_tax_position as Record<string, unknown>
  for (const missing of ["assessments", "initial_deferred_tax_asset", "restatements"]) {
    const invalid = { ...position }
    delete invalid[missing]
    assert.throws(() => parseIndustrialBooks({ ...books, income_tax_position: invalid }, "books"), new RegExp(missing))
  }
  assert.throws(() => parseIndustrialBooks({ ...books, income_tax_position: { ...position, assessment: null } }, "books"), /assessment/)
  assert.throws(() => parseIndustrialBooks({ ...books, income_tax_position: { ...position, assessments: { invalid: {} } } }, "books"), /assessments/)
})


test("报告新增字段仍严格拒绝缺失、未知 ROE 与旧公告 event 包装", { timeout: 10_000 }, () => {
  const report = fragments.report as Record<string, unknown>
  const reports = report.reports as Record<string, unknown>
  const { roe: _removed, ...missingRoe } = reports
  assert.throws(() => parseReportSet(missingRoe, "report"), /roe/)
  const income = reports.income as Record<string, unknown>
  for (const key of ["report_period", "report_period_net_income_to_parent"]) {
    const missing = { ...income }
    delete missing[key]
    assert.throws(() => parseReportSet({ ...reports, income: missing }, "report"), new RegExp(key))
  }
  assert.throws(() => parseReportSet({ ...reports, roe: { ...(reports.roe as Record<string, unknown>), extra: true } }, "report"), /roe/)
  const announcement = fragments.announcement as Record<string, unknown>
  const library = { next_seq: 8, reports: [], announcements: [announcement] }
  assert.deepEqual(parsePublicLibrary(library), library)
  const { content: _content, ...legacy } = announcement
  assert.throws(() => parsePublicLibrary({ ...library, announcements: [{ ...legacy, event: {} }] }), /content/)
})


test("公司历史语料仅在首次请求 fixture 时读取并缓存", { timeout: 10_000 }, async () => {
  const target = new URL("./fixtures/current-company-slice.json", import.meta.url)
  const originalReadFileSync = fs.readFileSync
  let targetReads = 0
  let capturedText: string | undefined

  fs.readFileSync = ((path: Parameters<typeof fs.readFileSync>[0], ...args: unknown[]) => {
    const result = (originalReadFileSync as (...values: unknown[]) => unknown)(path, ...args)
    if (path instanceof URL && path.href === target.href) {
      targetReads += 1
      assert.ok(typeof result === "string", "历史语料应以 utf8 文本读取")
      capturedText = result
    }
    return result
  }) as typeof fs.readFileSync

  try {
    syncBuiltinESMExports()
    const isolatedModuleUrl = new URL("./company-slice-test-fixture.ts?fixture-read-contract", import.meta.url)
    const isolatedModule = await import(isolatedModuleUrl.href)
    assert.equal(targetReads, 0, "导入模块不应读取历史语料")

    const minimalSource = { companies: {}, scheduler: { pending: [] }, payment_failures: {} }
    isolatedModule.projectHistoricalCompanyOperations(minimalSource)
    assert.equal(targetReads, 0, "纯 projection 不应读取历史语料")

    const first = isolatedModule.representativeCompanyOperationsFixture()
    assert.equal(targetReads, 1, "首次 fixture 请求应真实读取历史语料一次")
    const second = isolatedModule.representativeCompanyOperationsFixture()
    assert.equal(targetReads, 1, "后续 fixture 请求应复用缓存")
    assert.ok(typeof capturedText === "string", "应捕获真实文件读取文本")
    assert.deepEqual(first, isolatedModule.projectHistoricalCompanyOperations(JSON.parse(capturedText)))
    assert.deepEqual(second, first)
  } finally {
    fs.readFileSync = originalReadFileSync
    syncBuiltinESMExports()
  }
})
