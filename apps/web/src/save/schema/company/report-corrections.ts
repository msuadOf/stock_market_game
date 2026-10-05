import { SaveSchemaError, array, exact, record, string } from "../primitives.ts"
import { publicationId } from "../personal/common.ts"
import { parseJournalEntry, type JournalEntry } from "./accounting/journal.ts"
import { instant, type CivilInstantValue } from "./value.ts"
import type { parseCompanySystemState } from "./system.ts"
import type { PublicLibrary } from "./reports.ts"
import { accountingMinorUnits } from "./accounting/amount.ts"

export type CompanyReportCorrection = { readonly operation_id: string; readonly company: string; readonly supersedes: number; readonly entries: readonly JournalEntry[]; readonly reason: string }

export type CompletedReportCorrection = {
  readonly request: CompanyReportCorrection
  readonly publications: readonly number[]
  readonly committed_at: CivilInstantValue
}

function nonempty(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (parsed.trim().length === 0) throw new SaveSchemaError(path, "必须非空")
  return parsed
}

function entryFactKey(entry: JournalEntry): string {
  return JSON.stringify({ ...entry, lines: entry.lines.map((line) => ({ ...line, amount: accountingMinorUnits(line.amount).toString() })) })
}

function balancedEntry(value: unknown, path: string): JournalEntry {
  const parsed = parseJournalEntry(value, path)
  let debit = 0n
  let credit = 0n
  const maximum = (1n << 127n) - 1n
  for (const [index, line] of parsed.lines.entries()) {
    const amount = accountingMinorUnits(line.amount, `${path}.lines[${index}].amount`)
    if (amount <= 0n || amount > maximum) throw new SaveSchemaError(`${path}.lines[${index}].amount`, "必须为正i128会计金额（元）")
    if (line.side === "Debit") debit += amount
    else credit += amount
    if (debit > maximum || credit > maximum) throw new SaveSchemaError(path, "会计借贷合计超出i128范围")
  }
  if (debit === 0n || debit !== credit) throw new SaveSchemaError(path, "会计凭证须借贷两侧非空、正金额且平衡")
  return parsed
}

export function parseCompanyReportCorrection(value: unknown, path = "request"): CompanyReportCorrection {
  const request = record(value, path)
  exact(request, ["operation_id", "company", "supersedes", "entries", "reason"], path)
  const entries = array(request.entries, `${path}.entries`).map((entry, index) => balancedEntry(entry, `${path}.entries[${index}]`))
  if (entries.length === 0) throw new SaveSchemaError(`${path}.entries`, "须有实际凭证")
  return { operation_id: nonempty(request.operation_id, `${path}.operation_id`), company: nonempty(request.company, `${path}.company`), supersedes: publicationId(request.supersedes, `${path}.supersedes`), entries, reason: nonempty(request.reason, `${path}.reason`) }
}

export function parseCompletedReportCorrection(value: unknown, path = "completed"): CompletedReportCorrection {
  const item = record(value, path)
  exact(item, ["request", "publications", "committed_at"], path)
  const request = parseCompanyReportCorrection(item.request, `${path}.request`)
  const seen = new Set<number>()
  const publications = array(item.publications, `${path}.publications`).map((value, index) => {
    const id = publicationId(value, `${path}.publications[${index}]`)
    if (seen.has(id)) throw new SaveSchemaError(`${path}.publications[${index}]`, "真实公开ID不能重复归属更正操作")
    seen.add(id)
    return id
  })
  if (publications.length === 0) throw new SaveSchemaError(`${path}.publications`, "须有真实公开结果")
  return { request, publications, committed_at: instant(item.committed_at, `${path}.committed_at`) }
}

export function parseReportCorrections(value: unknown, path = "report_correction_operations"): Readonly<Record<string, CompletedReportCorrection>> {
  const source = record(value, path)
  const seen = new Set<number>()
  return Object.fromEntries(Object.entries(source).map(([identity, value]) => {
    const itemPath = `${path}.${identity}`
    nonempty(identity, itemPath)
    const item = parseCompletedReportCorrection(value, itemPath)
    if (item.request.operation_id !== identity) throw new SaveSchemaError(`${itemPath}.request.operation_id`, "必须等于operation身份键")
    for (const [index, id] of item.publications.entries()) {
      if (seen.has(id)) throw new SaveSchemaError(`${itemPath}.publications[${index}]`, "真实公开ID不能重复归属更正操作")
      seen.add(id)
    }
    return [identity, item]
  }))
}

export function validateReportCorrectionLinks(completed: Readonly<Record<string, CompletedReportCorrection>>, companySystem: ReturnType<typeof parseCompanySystemState>, library: PublicLibrary, currentDate: string): void {
  const reports = new Map(library.reports.map((report) => [report.id, report]))
  if (reports.size !== library.reports.length) throw new SaveSchemaError("public_library.reports", "公开报告ID重复")
  const journals = new Map<string, Map<number, string>>()
  const seen = new Set<number>()
  for (const [identity, item] of Object.entries(completed)) {
    const path = `report_correction_operations.${identity}`
    if (identity !== item.request.operation_id || identity.trim().length === 0 || item.request.reason.trim().length === 0 || item.request.entries.length === 0 || item.publications.length === 0) throw new SaveSchemaError(path, "日终更正操作身份、原因和结果必须完整")
    if (item.committed_at.date >= currentDate) throw new SaveSchemaError(`${path}.committed_at`, "必须属于已结束自然日")
    const original = reports.get(item.request.supersedes)
    const company = companySystem.implementation.state.companies[item.request.company]
    if (original === undefined || original.company !== item.request.company || !("Standalone" in original.reports.scope) || original.reports.scope.Standalone !== item.request.company || company === undefined || company.finance.company !== item.request.company) throw new SaveSchemaError(path, "更正必须关联真实公司财务与该公司原Standalone报告")
    if (original.source !== "SimpleGenerated") throw new SaveSchemaError(`${path}.request.supersedes`, "Simple 更正必须关联 Simple 原报告")
    if (original.published_at.date > item.committed_at.date || (original.published_at.date === item.committed_at.date && original.published_at.second_of_day > item.committed_at.second_of_day)) throw new SaveSchemaError(`${path}.request.supersedes`, "原报告在更正日终时点尚未公开")
    const finance = company.finance
    let journal = journals.get(item.request.company)
    if (journal === undefined) {
      journal = new Map()
      for (const entry of finance.books.journal.batches.flat()) {
        if (journal.has(entry.source)) throw new SaveSchemaError(`${path}.request.entries`, "公司凭证source重复")
        journal.set(entry.source, entryFactKey(entry))
      }
      journals.set(item.request.company, journal)
    }
    for (const entry of item.request.entries) {
      balancedEntry(entry, `${path}.request.entries`)
      const mappings = finance.closing.restatements.filter(([scope]) => "Standalone" in scope && scope.Standalone === item.request.company)
      const periods = mappings.flatMap(([, entries]) => entries).filter(([source]) => source === String(entry.source))
      if (journal.get(entry.source) !== entryFactKey(entry) || periods.length !== 1 || periods[0]![1] !== original.reports.period) throw new SaveSchemaError(`${path}.request.entries`, "必须关联实际owner凭证及原报告有效期间")
    }
    let linkedOriginal = false
    for (const id of item.publications) {
      if (seen.has(id)) throw new SaveSchemaError(`${path}.publications`, "真实公开ID不能重复归属更正操作")
      seen.add(id)
      const report = reports.get(id)
      if (report === undefined || report.origin !== "Correction" || report.published_at.date !== item.committed_at.date || report.published_at.second_of_day !== item.committed_at.second_of_day) throw new SaveSchemaError(`${path}.publications`, "必须关联同一日终真实更正公开结果")
      const standalone = report.company === item.request.company && JSON.stringify(report.reports.scope) === JSON.stringify(original.reports.scope)
      if (report.reports.period < original.reports.period || !standalone || report.source !== "SimpleGenerated") throw new SaveSchemaError(`${path}.publications`, "必须是原期间或后续期间同一公司Standalone的Simple更正")
      if (standalone && report.reports.period === original.reports.period && report.supersedes === item.request.supersedes) linkedOriginal = true
    }
    if (!linkedOriginal) throw new SaveSchemaError(`${path}.publications`, "须有实际替代原版的公司报告")
  }
}
