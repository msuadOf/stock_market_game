import { array, boolean, civilDate, exact, integer, map, nullable, record, string } from "../../primitives.ts"
import { parseBooks, type Books } from "../accounting/index.ts"
import { amount } from "../value.ts"
import { parseBudget, parseCounterpartyLedger, parseFraction, parseI128Number, parseTradeOpenLedger, type CounterpartyLedger, type OperatingBudget, type TradeOpenLedger } from "./common.ts"

export type RealEstateBooks = { readonly books: Books; readonly projects: Readonly<Record<string, ProjectState>>; readonly presales: Readonly<Record<string, PresaleContract>>; readonly loans: Readonly<Record<string, ProjectLoanState>>; readonly receivables: TradeOpenLedger; readonly counterparties: CounterpartyLedger; readonly budget: OperatingBudget; readonly capitalization_policy: CapitalizationPolicy; readonly max_projects: number; readonly next_event_id: number }
type ProjectState = { readonly total_units: number; readonly remaining_units: number; readonly land_cost: string; readonly development_cost: string; readonly capitalized_interest: string; readonly remaining_cost: string; readonly carried_out_cost: string; readonly dev_started_on: string | null; readonly interrupted_on: string | null; readonly interruptions: readonly Interruption[]; readonly completed_on: string | null }
type Interruption = { readonly start: string; readonly end: string }
type PresaleContract = { readonly project: string; readonly buyer: string; readonly units: number; readonly price_total: string; readonly collected: string; readonly delivered: boolean }
type ProjectLoanState = { readonly outstanding: string; readonly accrued_unpaid: string; readonly carried_cap: string; readonly carried_exp: string; readonly last_accrual_date: string; readonly lender: string; readonly project: string | null; readonly debt_account: string; readonly annual_rate_bp: number }
type CapitalizationPolicy = { readonly version: number; readonly suspension_min_days: number }

export function parseRealEstateBooks(value: unknown, path: string): RealEstateBooks {
  const parsed = record(value, path)
  exact(parsed, ["books", "projects", "presales", "loans", "receivables", "counterparties", "budget", "capitalization_policy", "max_projects", "next_event_id"], path)
  return { books: parseBooks(parsed.books, `${path}.books`), projects: map(parsed.projects, `${path}.projects`, stringKey, parseProject), presales: map(parsed.presales, `${path}.presales`, stringKey, parsePresale), loans: map(parsed.loans, `${path}.loans`, stringKey, parseLoan), receivables: parseTradeOpenLedger(parsed.receivables, `${path}.receivables`), counterparties: parseCounterpartyLedger(parsed.counterparties, `${path}.counterparties`), budget: parseBudget(parsed.budget, `${path}.budget`), capitalization_policy: parseCapitalizationPolicy(parsed.capitalization_policy, `${path}.capitalization_policy`), max_projects: integer(parsed.max_projects, `${path}.max_projects`, 0), next_event_id: integer(parsed.next_event_id, `${path}.next_event_id`, 0) }
}

function parseProject(value: unknown, path: string): ProjectState {
  const parsed = record(value, path)
  exact(parsed, ["total_units", "remaining_units", "land_cost", "development_cost", "capitalized_interest", "remaining_cost", "carried_out_cost", "dev_started_on", "interrupted_on", "interruptions", "completed_on"], path)
  return { total_units: parseI128Number(parsed.total_units, `${path}.total_units`), remaining_units: parseI128Number(parsed.remaining_units, `${path}.remaining_units`), land_cost: amount(parsed.land_cost, `${path}.land_cost`), development_cost: amount(parsed.development_cost, `${path}.development_cost`), capitalized_interest: amount(parsed.capitalized_interest, `${path}.capitalized_interest`), remaining_cost: amount(parsed.remaining_cost, `${path}.remaining_cost`), carried_out_cost: amount(parsed.carried_out_cost, `${path}.carried_out_cost`), dev_started_on: nullable(parsed.dev_started_on, `${path}.dev_started_on`, civilDate), interrupted_on: nullable(parsed.interrupted_on, `${path}.interrupted_on`, civilDate), interruptions: array(parsed.interruptions, `${path}.interruptions`).map((entry, index) => parseInterruption(entry, `${path}.interruptions[${index}]`)), completed_on: nullable(parsed.completed_on, `${path}.completed_on`, civilDate) }
}

function parseInterruption(value: unknown, path: string): Interruption {
  const parsed = record(value, path)
  exact(parsed, ["start", "end"], path)
  return { start: civilDate(parsed.start, `${path}.start`), end: civilDate(parsed.end, `${path}.end`) }
}

function parsePresale(value: unknown, path: string): PresaleContract {
  const parsed = record(value, path)
  exact(parsed, ["project", "buyer", "units", "price_total", "collected", "delivered"], path)
  return { project: string(parsed.project, `${path}.project`), buyer: string(parsed.buyer, `${path}.buyer`), units: parseI128Number(parsed.units, `${path}.units`), price_total: amount(parsed.price_total, `${path}.price_total`), collected: amount(parsed.collected, `${path}.collected`), delivered: boolean(parsed.delivered, `${path}.delivered`) }
}

function parseLoan(value: unknown, path: string): ProjectLoanState {
  const parsed = record(value, path)
  exact(parsed, ["outstanding", "accrued_unpaid", "carried_cap", "carried_exp", "last_accrual_date", "lender", "project", "debt_account", "annual_rate_bp"], path)
  return { outstanding: amount(parsed.outstanding, `${path}.outstanding`), accrued_unpaid: amount(parsed.accrued_unpaid, `${path}.accrued_unpaid`), carried_cap: parseFraction(parsed.carried_cap, `${path}.carried_cap`), carried_exp: parseFraction(parsed.carried_exp, `${path}.carried_exp`), last_accrual_date: civilDate(parsed.last_accrual_date, `${path}.last_accrual_date`), lender: string(parsed.lender, `${path}.lender`), project: nullable(parsed.project, `${path}.project`, string), debt_account: string(parsed.debt_account, `${path}.debt_account`), annual_rate_bp: integer(parsed.annual_rate_bp, `${path}.annual_rate_bp`) }
}

function parseCapitalizationPolicy(value: unknown, path: string): CapitalizationPolicy {
  const parsed = record(value, path)
  exact(parsed, ["version", "suspension_min_days"], path)
  return { version: integer(parsed.version, `${path}.version`, 0), suspension_min_days: integer(parsed.suspension_min_days, `${path}.suspension_min_days`) }
}
function stringKey(key: string): void { string(key, key) }
