import { readFileSync } from "node:fs"

const operations = JSON.parse(readFileSync(new URL("./fixtures/current-company-slice.json", import.meta.url), "utf8")) as Record<string, unknown>

/** 历史账套仅用于 schema projection，不声明其新增税务事实可由 Rust 恢复。 */
export function projectHistoricalCompanyOperations(source: Record<string, unknown>): Record<string, unknown> {
  const companies = source.companies as Record<string, Record<string, unknown>>
  const codes = Object.keys(companies).sort()
  const companyId = (code: unknown): number => {
    const id = codes.indexOf(String(code))
    if (id < 0) throw new Error(`历史测试语料公司不存在：${String(code)}`)
    return id
  }
  const catalog = { metas: codes.map((code) => ({ code, name: String((companies[code]!.spec as Record<string, unknown>).name) })) }
  const projectedCompanies = Object.fromEntries(codes.map((code, id) => {
    const company = companies[code]!
    const { id: _removed, issued_shares, ...spec } = company.spec as Record<string, unknown>
    const books = company.books as Record<string, Record<string, unknown>>
    const industrial = books.Industrial!
    const tax = industrial.income_tax_position as { loss_pool: unknown; assessment: { year: number; pretax: string; current_tax: string; opening_loss_pool: unknown } | null }
    const assessments = tax.assessment === null ? {} : { [tax.assessment.year]: { pretax: tax.assessment.pretax, current_tax: tax.assessment.current_tax, opening_loss_pool: tax.assessment.opening_loss_pool, deferred_tax_asset: "0.00" } }
    return [String(id), { ...company, spec: { ...spec, code, issued_shares: String(issued_shares) }, books: { Industrial: { ...industrial, income_tax_position: { loss_pool: tax.loss_pool, assessments, initial_deferred_tax_asset: "0.00", restatements: {} } } } }]
  }))
  const scheduler = source.scheduler as { pending: { action: Record<string, Record<string, unknown>> }[] }
  const pending = scheduler.pending.map((due) => ({ ...due, action: Object.fromEntries(Object.entries(due.action).map(([kind, action]) => [kind, { ...action, company: companyId(action.company) }])) }))
  const payment_failures = Object.fromEntries(Object.entries(source.payment_failures as Record<string, Record<string, unknown>[]>).map(([date, rows]) => [date, rows.map((row) => ({ ...row, company: companyId(row.company) }))]))
  return { ...source, catalog, companies: projectedCompanies, scheduler: { ...scheduler, pending }, payment_failures }
}

export function representativeCompanyOperationsFixture(): Record<string, unknown> {
  return projectHistoricalCompanyOperations(operations)
}
