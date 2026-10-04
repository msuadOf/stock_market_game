import { SaveSchemaError, array, civilDate, exact, integer, record, string } from "../primitives.ts"
import { parseCompanySpec } from "./accounting/index.ts"
import { parseIndustryBooks, type IndustryBooks } from "./books/index.ts"
import { parseActiveShocks, parseFlowParams, parseHistory, parseScheduler, parseShockParams, type ActiveShock, type FlowParams, type History, type Scheduler, type ShockParams } from "./policies/index.ts"
import { instant, nullableDate, u64, type CivilInstantValue, type U64 } from "./value.ts"
import { parsePaymentFailure, type PaymentFailure } from "./policies/shock.ts"

export type CompanyOperationsConfig = { readonly seed: number; readonly shock_params: ShockParams; readonly companies: readonly { readonly spec: ReturnType<typeof parseCompanySpec>; readonly books: IndustryBooks; readonly flow: FlowParams }[] }

export function parseCompanyOperationsConfig(value: unknown, path: string): CompanyOperationsConfig {
  const parsed = record(value, path)
  exact(parsed, ["seed", "shock_params", "companies"], path)
  return { seed: integer(parsed.seed, `${path}.seed`, 0), shock_params: parseShockParams(parsed.shock_params, `${path}.shock_params`), companies: array(parsed.companies, `${path}.companies`).map((value, index) => {
    const itemPath = `${path}.companies[${index}]`
    const item = record(value, itemPath)
    exact(item, ["spec", "books", "flow"], itemPath)
    return { spec: parseCompanySpec(item.spec, `${itemPath}.spec`), books: parseIndustryBooks(item.books, `${itemPath}.books`), flow: parseFlowParams(item.flow, `${itemPath}.flow`) }
  }) }
}

export type OperatingRng = { readonly state: U64 }
export type OperatingCompany = {
  readonly spec: ReturnType<typeof parseCompanySpec>
  readonly books: IndustryBooks
  readonly params: FlowParams
  readonly rng: OperatingRng
  readonly economy: { readonly active: readonly ActiveShock[] }
  readonly next_flow_seq: number
}
export type CompanyOperations = {
  readonly seed: U64
  readonly shock_params: ShockParams
  readonly scheduler: Scheduler
  readonly market_rng: OperatingRng
  readonly industry_rngs: Readonly<Record<string, OperatingRng>>
  readonly companies: Readonly<Record<string, OperatingCompany>>
  readonly next_expected: string | null
  readonly history: History | null
  readonly payment_failures: Readonly<Record<string, readonly (PaymentFailure & { readonly company: string })[]>>
}
export type OperationsWiring = { readonly mirrored: readonly number[] }
export type DisclosureDispatch = { readonly published_through: CivilInstantValue | null; readonly announced_through: string | null }

function parseRng(value: unknown, path: string): OperatingRng {
  const parsed = record(value, path)
  exact(parsed, ["state"], path)
  return { state: u64(parsed.state, `${path}.state`) }
}

function parseCompany(value: unknown, path: string): OperatingCompany {
  const parsed = record(value, path)
  exact(parsed, ["spec", "books", "params", "rng", "economy", "next_flow_seq"], path)
  const economy = record(parsed.economy, `${path}.economy`)
  exact(economy, ["active"], `${path}.economy`)
  return {
    spec: parseCompanySpec(parsed.spec, `${path}.spec`),
    books: parseIndustryBooks(parsed.books, `${path}.books`),
    params: parseFlowParams(parsed.params, `${path}.params`),
    rng: parseRng(parsed.rng, `${path}.rng`),
    economy: { active: parseActiveShocks(economy.active, `${path}.economy.active`) },
    next_flow_seq: integer(parsed.next_flow_seq, `${path}.next_flow_seq`, 0),
  }
}

export function parseCompanyOperations(value: unknown, path = "company_operations"): CompanyOperations {
  const parsed = record(value, path)
  exact(parsed, ["seed", "shock_params", "scheduler", "market_rng", "industry_rngs", "companies", "next_expected", "history", "payment_failures"], path)
  const industryRngs = record(parsed.industry_rngs, `${path}.industry_rngs`)
  const companies = record(parsed.companies, `${path}.companies`)
  const parsedIndustryRngs: Record<string, OperatingRng> = {}
  for (const [key, nested] of Object.entries(industryRngs)) parsedIndustryRngs[key] = parseRng(nested, `${path}.industry_rngs.${key}`)
  const parsedCompanies: Record<string, OperatingCompany> = {}
  for (const [key, nested] of Object.entries(companies)) parsedCompanies[key] = parseCompany(nested, `${path}.companies.${key}`)
  const failures: Record<string, readonly (PaymentFailure & { readonly company: string })[]> = {}
  const nextExpected = nullableDate(parsed.next_expected, `${path}.next_expected`)
  for (const [date, value] of Object.entries(record(parsed.payment_failures, `${path}.payment_failures`))) {
    const datePath = `${path}.payment_failures.${date}`
    civilDate(date, datePath)
    if (nextExpected === null || date >= nextExpected) throw new SaveSchemaError(datePath, "必须早于 next_expected")
    const entries = array(value, datePath)
    if (entries.length === 0) throw new SaveSchemaError(datePath, "失败记录不得为空")
    failures[date] = entries.map((value, index) => {
      const itemPath = `${datePath}[${index}]`
      const item = record(value, itemPath)
      exact(item, ["company", "what", "amount", "obligation_status"], itemPath)
      const company = string(item.company, `${itemPath}.company`)
      if (!Object.hasOwn(parsedCompanies, company)) throw new SaveSchemaError(`${itemPath}.company`, "公司不存在")
      return { company, ...parsePaymentFailure({ what: item.what, amount: item.amount, obligation_status: item.obligation_status }, itemPath) }
    })
  }
  return {
    seed: u64(parsed.seed, `${path}.seed`),
    shock_params: parseShockParams(parsed.shock_params, `${path}.shock_params`),
    scheduler: parseScheduler(parsed.scheduler, `${path}.scheduler`),
    market_rng: parseRng(parsed.market_rng, `${path}.market_rng`),
    industry_rngs: parsedIndustryRngs,
    companies: parsedCompanies,
    next_expected: nextExpected,
    history: parsed.history === null ? null : parseHistory(parsed.history, `${path}.history`),
    payment_failures: failures,
  }
}

export function parseOperationsWiring(value: unknown, path = "ops_wiring"): OperationsWiring {
  const parsed = record(value, path)
  exact(parsed, ["mirrored"], path)
  return { mirrored: array(parsed.mirrored, `${path}.mirrored`).map((entry, index) => integer(entry, `${path}.mirrored[${index}]`, 0)) }
}

export function parseDisclosureDispatch(value: unknown, path = "disclosures"): DisclosureDispatch {
  const parsed = record(value, path)
  exact(parsed, ["published_through", "announced_through"], path)
  return {
    published_through: parsed.published_through === null ? null : instant(parsed.published_through, `${path}.published_through`),
    announced_through: nullableDate(parsed.announced_through, `${path}.announced_through`),
  }
}
