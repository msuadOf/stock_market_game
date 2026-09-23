import { SaveSchemaError, array, civilDate, exact, integer, map, oneOf, record, string } from "../../primitives.ts"
import type { Books } from "../accounting/index.ts"
import { amount, i128 } from "../value.ts"

export type { Books }
export type TradeOpenLedger = { readonly items: Readonly<Record<string, OpenItem>>; readonly written_off_total: string }
export type OpenItem = { readonly party: string; readonly opened_on: string; readonly due_on: string; readonly open_amount: string }
export type CounterpartyLedger = { readonly counterparties: Readonly<Record<string, ExternalCounterparty>>; readonly flows: readonly CounterpartyFlow[] }
export type ExternalCounterparty = { readonly id: string; readonly kind: CounterpartyKind; readonly name: string }
export type CounterpartyFlow = { readonly date: string; readonly counterparty: string; readonly direction: FlowDirection; readonly amount: string; readonly memo: string }
export type CounterpartyKind = "Customer" | "Supplier" | "Employee" | "TaxAuthority" | "Lender"
export type FlowDirection = "Inbound" | "Outbound"
export type OperatingBudget = { readonly operating_cash_floor: string; readonly credit_lines: Readonly<Record<string, string>> }
export type ContractBook = { readonly contracts: Readonly<Record<string, OperatingContract>> }
export type OperatingContract = { readonly id: string; readonly role: ContractRole; readonly counterparty: string; readonly principal: string; readonly annual_rate_bp: number; readonly start_date: string; readonly maturity_date: string; readonly basis: "Act365F" }
export type ContractRole = "Borrowing" | "Payable" | "Receivable"

const counterpartyKinds = ["Customer", "Supplier", "Employee", "TaxAuthority", "Lender"] as const
const flowDirections = ["Inbound", "Outbound"] as const
const contractRoles = ["Borrowing", "Payable", "Receivable"] as const

export function parseTradeOpenLedger(value: unknown, path: string): TradeOpenLedger {
  const parsed = record(value, path)
  exact(parsed, ["items", "written_off_total"], path)
  return { items: map(parsed.items, `${path}.items`, stringKey, parseOpenItem), written_off_total: amount(parsed.written_off_total, `${path}.written_off_total`) }
}

function parseOpenItem(value: unknown, path: string): OpenItem {
  const parsed = record(value, path)
  exact(parsed, ["party", "opened_on", "due_on", "open_amount"], path)
  return { party: string(parsed.party, `${path}.party`), opened_on: civilDate(parsed.opened_on, `${path}.opened_on`), due_on: civilDate(parsed.due_on, `${path}.due_on`), open_amount: amount(parsed.open_amount, `${path}.open_amount`) }
}

export function parseCounterpartyLedger(value: unknown, path: string): CounterpartyLedger {
  const parsed = record(value, path)
  exact(parsed, ["counterparties", "flows"], path)
  return { counterparties: map(parsed.counterparties, `${path}.counterparties`, stringKey, parseCounterparty), flows: array(parsed.flows, `${path}.flows`).map((flow, index) => parseFlow(flow, `${path}.flows[${index}]`)) }
}

function parseCounterparty(value: unknown, path: string): ExternalCounterparty {
  const parsed = record(value, path)
  exact(parsed, ["id", "kind", "name"], path)
  return { id: string(parsed.id, `${path}.id`), kind: oneOf(parsed.kind, `${path}.kind`, counterpartyKinds), name: string(parsed.name, `${path}.name`) }
}

function parseFlow(value: unknown, path: string): CounterpartyFlow {
  const parsed = record(value, path)
  exact(parsed, ["date", "counterparty", "direction", "amount", "memo"], path)
  return { date: civilDate(parsed.date, `${path}.date`), counterparty: string(parsed.counterparty, `${path}.counterparty`), direction: oneOf(parsed.direction, `${path}.direction`, flowDirections), amount: amount(parsed.amount, `${path}.amount`), memo: string(parsed.memo, `${path}.memo`) }
}

export function parseBudget(value: unknown, path: string): OperatingBudget {
  const parsed = record(value, path)
  exact(parsed, ["operating_cash_floor", "credit_lines"], path)
  return { operating_cash_floor: amount(parsed.operating_cash_floor, `${path}.operating_cash_floor`), credit_lines: map(parsed.credit_lines, `${path}.credit_lines`, stringKey, amount) }
}

export function parseContractBook(value: unknown, path: string): ContractBook {
  const parsed = record(value, path)
  exact(parsed, ["contracts"], path)
  return { contracts: map(parsed.contracts, `${path}.contracts`, stringKey, parseContract) }
}

function parseContract(value: unknown, path: string): OperatingContract {
  const parsed = record(value, path)
  exact(parsed, ["id", "role", "counterparty", "principal", "annual_rate_bp", "start_date", "maturity_date", "basis"], path)
  return { id: string(parsed.id, `${path}.id`), role: oneOf(parsed.role, `${path}.role`, contractRoles), counterparty: string(parsed.counterparty, `${path}.counterparty`), principal: amount(parsed.principal, `${path}.principal`), annual_rate_bp: integer(parsed.annual_rate_bp, `${path}.annual_rate_bp`), start_date: civilDate(parsed.start_date, `${path}.start_date`), maturity_date: civilDate(parsed.maturity_date, `${path}.maturity_date`), basis: oneOf(parsed.basis, `${path}.basis`, ["Act365F"] as const) }
}

export function parseFraction(value: unknown, path: string): string { return i128(value, path) }
export function parseI128Number(value: unknown, path: string): number {
  return integer(value, path)
}
function stringKey(key: string): void { if (key.length === 0) throw new SaveSchemaError(key, "映射键不能为空") }
