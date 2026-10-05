import { SaveSchemaError, array, exact, integer, map, record, safeIntegerKey } from "../../primitives.ts"
import { amount, period } from "../value.ts"
import { accountingMinorUnits } from "../accounting/amount.ts"
import type { Books } from "../accounting/books.ts"

export type IncomeTaxPolicy = { readonly rate_bp: number; readonly loss_carryforward_years: number }
export type LossEntry = { readonly origin_year: number; readonly remaining: string }
export type IncomeTaxAssessment = { readonly pretax: string; readonly current_tax: string; readonly opening_loss_pool: readonly LossEntry[]; readonly deferred_tax_asset: string }
export type IncomeTaxPosition = { readonly loss_pool: readonly LossEntry[]; readonly assessments: Readonly<Record<string, IncomeTaxAssessment>>; readonly initial_deferred_tax_asset: string; readonly restatements: Readonly<Record<string, string>> }

function checked(value: bigint, path: string): bigint {
  if (value < -(1n << 127n) || value > (1n << 127n) - 1n) throw new SaveSchemaError(path, "税务会计运算超出i128范围")
  return value
}

function taxAmount(value: unknown, path: string, nonnegative = false): string {
  const parsed = amount(value, path)
  if (accountingMinorUnits(parsed, path) < 0n && nonnegative) throw new SaveSchemaError(path, "税额或递延税资产不得为负")
  return parsed
}

export function parseIncomeTaxPolicy(value: unknown, path: string): IncomeTaxPolicy {
  const parsed = record(value, path)
  exact(parsed, ["rate_bp", "loss_carryforward_years"], path)
  const rate = integer(parsed.rate_bp, `${path}.rate_bp`, 0)
  const years = integer(parsed.loss_carryforward_years, `${path}.loss_carryforward_years`, 1)
  if (rate > 10000 || years > 65535) throw new SaveSchemaError(path, "税率须在0–10000基点，结转年限须为正u16")
  return { rate_bp: rate, loss_carryforward_years: years }
}

function parsePool(value: unknown, path: string): readonly LossEntry[] {
  let previous = 1899
  return array(value, path).map((value, index) => {
    const itemPath = `${path}[${index}]`
    const item = record(value, itemPath)
    exact(item, ["origin_year", "remaining"], itemPath)
    const year = integer(item.origin_year, `${itemPath}.origin_year`, 1900)
    const remaining = taxAmount(item.remaining, `${itemPath}.remaining`)
    if (year > 2199 || year <= previous || accountingMinorUnits(remaining) <= 0n) throw new SaveSchemaError(itemPath, "亏损池须按有效起源年递增且金额为正")
    previous = year
    return { origin_year: year, remaining }
  })
}

function yearKey(key: string, path: string): void {
  if (!/^(19|20|21)\d{2}$/.test(key)) throw new SaveSchemaError(path, "必须为1900–2199规范税务年度键")
}

function assess(value: unknown, path: string): IncomeTaxAssessment {
  const item = record(value, path)
  exact(item, ["pretax", "current_tax", "opening_loss_pool", "deferred_tax_asset"], path)
  return { pretax: taxAmount(item.pretax, `${path}.pretax`), current_tax: taxAmount(item.current_tax, `${path}.current_tax`, true), opening_loss_pool: parsePool(item.opening_loss_pool, `${path}.opening_loss_pool`), deferred_tax_asset: taxAmount(item.deferred_tax_asset, `${path}.deferred_tax_asset`, true) }
}

function samePool(first: readonly LossEntry[], second: readonly LossEntry[]): boolean {
  return first.length === second.length && first.every((entry, index) => entry.origin_year === second[index].origin_year && accountingMinorUnits(entry.remaining) === accountingMinorUnits(second[index].remaining))
}

function basisPoints(value: bigint, rate: number, path: string): bigint {
  const scaled = checked(value * BigInt(rate), path)
  const quotient = scaled / 10000n
  const twice = scaled % 10000n * 2n
  return quotient + (twice > 10000n || (twice === 10000n && quotient % 2n !== 0n) ? 1n : 0n)
}

function validateChain(position: IncomeTaxPosition, policy: IncomeTaxPolicy, path: string): void {
  let previousYear: number | null = null
  let previousPool: readonly LossEntry[] | null = null
  for (const [yearText, assessment] of Object.entries(position.assessments).sort(([first], [second]) => Number(first) - Number(second))) {
    const year = Number(yearText)
    const currentPath = `${path}.assessments.${yearText}`
    if (assessment.opening_loss_pool.some((entry) => entry.origin_year >= year)) throw new SaveSchemaError(currentPath, "期初亏损池不得包含本年或未来亏损")
    if (previousYear !== null && (year !== previousYear + 1 || previousPool === null || !samePool(previousPool, assessment.opening_loss_pool))) throw new SaveSchemaError(currentPath, "年度评估年份及期初期末亏损池必须连续")
    let expired = 0n
    const pool = assessment.opening_loss_pool.filter((entry) => {
      if (year - entry.origin_year <= policy.loss_carryforward_years) return true
      expired = checked(expired + accountingMinorUnits(entry.remaining), currentPath)
      return false
    }).map((entry) => ({ origin_year: entry.origin_year, remaining: accountingMinorUnits(entry.remaining) }))
    const pretax = accountingMinorUnits(assessment.pretax)
    let taxable = pretax > 0n ? pretax : 0n
    for (const entry of pool) {
      const used = entry.remaining < taxable ? entry.remaining : taxable
      entry.remaining -= used
      taxable -= used
    }
    const ending = pool.filter((entry) => entry.remaining > 0n)
    if (pretax < 0n) ending.push({ origin_year: year, remaining: checked(-pretax, currentPath) })
    const remaining = ending.reduce((total, entry) => checked(total + entry.remaining, currentPath), 0n)
    if (basisPoints(taxable, policy.rate_bp, currentPath) !== accountingMinorUnits(assessment.current_tax) || basisPoints(remaining, policy.rate_bp, currentPath) !== accountingMinorUnits(assessment.deferred_tax_asset)) throw new SaveSchemaError(currentPath, "税额与递延税资产不符合真实年初基准和半偶舍入")
    previousYear = year
    previousPool = ending.map((entry) => ({ origin_year: entry.origin_year, remaining: `${entry.remaining / 100n}.${(entry.remaining % 100n).toString().padStart(2, "0")}` }))
  }
  if (previousPool !== null && !samePool(previousPool, position.loss_pool)) throw new SaveSchemaError(`${path}.loss_pool`, "最终亏损池与年度评估链不符")
}

function validateBooks(position: IncomeTaxPosition, books: Books, path: string): void {
  const taxAccounts = ["1811", "222104", "6801"]
  for (const [code, element] of [["1811", "Asset"], ["222104", "Liability"], ["6801", "Expense"]]) {
    const definition = books.chart.accounts[code]
    if (definition === undefined || definition.element !== element || definition.is_cash || definition.is_contra) throw new SaveSchemaError(path, `税务Owner科目${code}要素、现金或备抵语义不正确`)
  }
  let openingDta = 0n
  let actualDta = 0n
  let accruedTax = 0n
  let hasAccrual = false
  const dates = new Map<number, string>()
  for (const entry of books.journal.batches.flat()) {
    dates.set(entry.source, entry.date.slice(0, 7))
    const taxLines = entry.lines.filter((line) => taxAccounts.includes(line.account))
    for (const line of taxLines) {
      const value = accountingMinorUnits(line.amount, path)
      if (value <= 0n) throw new SaveSchemaError(path, "税务凭证金额必须为正")
      if (line.account === "1811") {
        const delta = line.side === "Debit" ? value : -value
        actualDta = checked(actualDta + delta, path)
        if (entry.kind === "OpeningBalance") openingDta = checked(openingDta + delta, path)
      }
      if (entry.kind === "TaxAccrual" && line.account === "222104") accruedTax = checked(accruedTax + (line.side === "Credit" ? value : -value), path)
    }
    if (taxLines.length === 0 || entry.kind === "OpeningBalance") continue
    if (entry.kind === "TaxAccrual") {
      hasAccrual = true
      if (entry.cash_flow !== "NonCash" || taxLines.length !== entry.lines.length) throw new SaveSchemaError(path, "所得税计提须为纯税务NonCash凭证")
    } else if (entry.kind === "TaxPayment") {
      if (entry.cash_flow !== "Operating" || taxLines.some((line) => line.account !== "222104" || line.side !== "Debit") || entry.lines.some((line) => line.account !== "222104" && (line.side !== "Credit" || !books.chart.accounts[line.account]?.is_cash))) throw new SaveSchemaError(path, "所得税缴纳只能Dr应交所得税、Cr真实现金，不能退款或非现金抵税")
    } else throw new SaveSchemaError(path, "税务Owner凭证不能隐藏为普通经营业务")
  }
  if (openingDta !== accountingMinorUnits(position.initial_deferred_tax_asset)) throw new SaveSchemaError(path, "初始递延税资产与OpeningBalance不符")
  for (const [source, target] of Object.entries(position.restatements)) {
    const actual = dates.get(Number(source))
    if (actual === undefined || target >= actual) throw new SaveSchemaError(`${path}.restatements.${source}`, "必须关联实际owner凭证并作用于实际过账期间之前")
  }
  const assessments = Object.entries(position.assessments).sort(([first], [second]) => Number(first) - Number(second))
  if (assessments.length === 0) {
    if (actualDta !== openingDta || hasAccrual) throw new SaveSchemaError(path, "空年度评估不能隐藏税务计提或递延税资产变动")
  } else {
    const expectedTax = assessments.reduce((total, [, entry]) => checked(total + accountingMinorUnits(entry.current_tax), path), 0n)
    if (actualDta !== accountingMinorUnits(assessments[assessments.length - 1][1].deferred_tax_asset) || accruedTax !== expectedTax) throw new SaveSchemaError(path, "最新递延税资产或累计所得税计提与GL不符")
  }
}

export function parseIncomeTaxPosition(value: unknown, path: string, policy: IncomeTaxPolicy, books: Books): IncomeTaxPosition {
  const item = record(value, path)
  exact(item, ["loss_pool", "assessments", "initial_deferred_tax_asset", "restatements"], path)
  const parsed = { loss_pool: parsePool(item.loss_pool, `${path}.loss_pool`), assessments: map(item.assessments, `${path}.assessments`, yearKey, assess), initial_deferred_tax_asset: taxAmount(item.initial_deferred_tax_asset, `${path}.initial_deferred_tax_asset`, true), restatements: map(item.restatements, `${path}.restatements`, safeIntegerKey, period) }
  validateChain(parsed, policy, path)
  validateBooks(parsed, books, path)
  return parsed
}
