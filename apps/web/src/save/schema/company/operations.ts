import { array, exact, integer, record } from "../primitives.ts"
import { parseCompanySpec } from "./accounting/index.ts"
import { parseIndustryBooks, type IndustryBooks } from "./books/index.ts"
import { parseActiveShocks, parseFlowParams, parseHistory, parseScheduler, parseShockParams, type ActiveShock, type FlowParams, type History, type Scheduler, type ShockParams } from "./policies/index.ts"
import { instant, nullableDate, u64, type CivilInstantValue, type U64 } from "./value.ts"

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
  exact(parsed, ["seed", "shock_params", "scheduler", "market_rng", "industry_rngs", "companies", "next_expected", "history"], path)
  const industryRngs = record(parsed.industry_rngs, `${path}.industry_rngs`)
  const companies = record(parsed.companies, `${path}.companies`)
  const parsedIndustryRngs: Record<string, OperatingRng> = {}
  for (const [key, nested] of Object.entries(industryRngs)) parsedIndustryRngs[key] = parseRng(nested, `${path}.industry_rngs.${key}`)
  const parsedCompanies: Record<string, OperatingCompany> = {}
  for (const [key, nested] of Object.entries(companies)) parsedCompanies[key] = parseCompany(nested, `${path}.companies.${key}`)
  return {
    seed: u64(parsed.seed, `${path}.seed`),
    shock_params: parseShockParams(parsed.shock_params, `${path}.shock_params`),
    scheduler: parseScheduler(parsed.scheduler, `${path}.scheduler`),
    market_rng: parseRng(parsed.market_rng, `${path}.market_rng`),
    industry_rngs: parsedIndustryRngs,
    companies: parsedCompanies,
    next_expected: nullableDate(parsed.next_expected, `${path}.next_expected`),
    history: parsed.history === null ? null : parseHistory(parsed.history, `${path}.history`),
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
