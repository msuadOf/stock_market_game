import { array, civilDate, exact, integer, map, record, string } from "../../primitives.ts"
import { parseBooks, type Books } from "../accounting/index.ts"
import { amount } from "../value.ts"
import { parseBudget, parseContractBook, parseCounterpartyLedger, parseFraction, parseI128Number, parseTradeOpenLedger, type ContractBook, type CounterpartyLedger, type OperatingBudget, type TradeOpenLedger } from "./common.ts"

export type IndustrialBooks = { readonly books: Books; readonly inventory: InventoryLedger; readonly assets: FixedAssetRegister; readonly receivables: TradeOpenLedger; readonly payables: TradeOpenLedger; readonly contracts: ContractBook; readonly counterparties: CounterpartyLedger; readonly budget: OperatingBudget; readonly tax_policy: TaxPolicy; readonly loss_pool: readonly LossEntry[]; readonly loans: Readonly<Record<string, IndustrialLoanState>>; readonly next_event_id: number }
type InventoryLedger = { readonly items: Readonly<Record<string, InventoryItemState>> }
type InventoryItemState = { readonly account: string; readonly quantity: number; readonly total_cost: string }
type FixedAssetRegister = { readonly assets: Readonly<Record<string, FixedAssetEntry>> }
type FixedAssetEntry = { readonly cost: string; readonly salvage_value: string; readonly life_months: number; readonly depreciated_months: number; readonly accumulated_depreciation: string; readonly accumulated_impairment: string }
type TaxPolicy = { readonly version: number; readonly vat: VatPolicy; readonly income_tax: IncomeTaxPolicy }
type VatPolicy = { readonly output_rate_bp: number; readonly input_rate_bp: number; readonly deductible_share_bp: number }
type IncomeTaxPolicy = { readonly rate_bp: number; readonly loss_carryforward_years: number }
type LossEntry = { readonly origin_year: number; readonly remaining: string }
type IndustrialLoanState = { readonly outstanding: string; readonly accrued_unpaid: string; readonly carried: string; readonly last_accrual_date: string }

export function parseIndustrialBooks(value: unknown, path: string): IndustrialBooks {
  const parsed = record(value, path)
  exact(parsed, ["books", "inventory", "assets", "receivables", "payables", "contracts", "counterparties", "budget", "tax_policy", "loss_pool", "loans", "next_event_id"], path)
  return { books: parseBooks(parsed.books, `${path}.books`), inventory: parseInventory(parsed.inventory, `${path}.inventory`), assets: parseAssets(parsed.assets, `${path}.assets`), receivables: parseTradeOpenLedger(parsed.receivables, `${path}.receivables`), payables: parseTradeOpenLedger(parsed.payables, `${path}.payables`), contracts: parseContractBook(parsed.contracts, `${path}.contracts`), counterparties: parseCounterpartyLedger(parsed.counterparties, `${path}.counterparties`), budget: parseBudget(parsed.budget, `${path}.budget`), tax_policy: parseTaxPolicy(parsed.tax_policy, `${path}.tax_policy`), loss_pool: array(parsed.loss_pool, `${path}.loss_pool`).map((entry, index) => parseLossEntry(entry, `${path}.loss_pool[${index}]`)), loans: map(parsed.loans, `${path}.loans`, stringKey, parseLoan), next_event_id: integer(parsed.next_event_id, `${path}.next_event_id`, 0) }
}

function parseInventory(value: unknown, path: string): InventoryLedger {
  const parsed = record(value, path)
  exact(parsed, ["items"], path)
  return { items: map(parsed.items, `${path}.items`, stringKey, parseInventoryItem) }
}

function parseInventoryItem(value: unknown, path: string): InventoryItemState {
  const parsed = record(value, path)
  exact(parsed, ["account", "quantity", "total_cost"], path)
  return { account: string(parsed.account, `${path}.account`), quantity: parseI128Number(parsed.quantity, `${path}.quantity`), total_cost: amount(parsed.total_cost, `${path}.total_cost`) }
}

function parseAssets(value: unknown, path: string): FixedAssetRegister {
  const parsed = record(value, path)
  exact(parsed, ["assets"], path)
  return { assets: map(parsed.assets, `${path}.assets`, stringKey, parseAsset) }
}

function parseAsset(value: unknown, path: string): FixedAssetEntry {
  const parsed = record(value, path)
  exact(parsed, ["cost", "salvage_value", "life_months", "depreciated_months", "accumulated_depreciation", "accumulated_impairment"], path)
  return { cost: amount(parsed.cost, `${path}.cost`), salvage_value: amount(parsed.salvage_value, `${path}.salvage_value`), life_months: integer(parsed.life_months, `${path}.life_months`), depreciated_months: integer(parsed.depreciated_months, `${path}.depreciated_months`, 0), accumulated_depreciation: amount(parsed.accumulated_depreciation, `${path}.accumulated_depreciation`), accumulated_impairment: amount(parsed.accumulated_impairment, `${path}.accumulated_impairment`) }
}

function parseTaxPolicy(value: unknown, path: string): TaxPolicy {
  const parsed = record(value, path)
  exact(parsed, ["version", "vat", "income_tax"], path)
  return { version: integer(parsed.version, `${path}.version`, 0), vat: parseVat(parsed.vat, `${path}.vat`), income_tax: parseIncomeTax(parsed.income_tax, `${path}.income_tax`) }
}

function parseVat(value: unknown, path: string): VatPolicy {
  const parsed = record(value, path)
  exact(parsed, ["output_rate_bp", "input_rate_bp", "deductible_share_bp"], path)
  return { output_rate_bp: integer(parsed.output_rate_bp, `${path}.output_rate_bp`), input_rate_bp: integer(parsed.input_rate_bp, `${path}.input_rate_bp`), deductible_share_bp: integer(parsed.deductible_share_bp, `${path}.deductible_share_bp`) }
}

function parseIncomeTax(value: unknown, path: string): IncomeTaxPolicy {
  const parsed = record(value, path)
  exact(parsed, ["rate_bp", "loss_carryforward_years"], path)
  return { rate_bp: integer(parsed.rate_bp, `${path}.rate_bp`), loss_carryforward_years: integer(parsed.loss_carryforward_years, `${path}.loss_carryforward_years`, 0) }
}

function parseLossEntry(value: unknown, path: string): LossEntry {
  const parsed = record(value, path)
  exact(parsed, ["origin_year", "remaining"], path)
  return { origin_year: integer(parsed.origin_year, `${path}.origin_year`), remaining: amount(parsed.remaining, `${path}.remaining`) }
}

function parseLoan(value: unknown, path: string): IndustrialLoanState {
  const parsed = record(value, path)
  exact(parsed, ["outstanding", "accrued_unpaid", "carried", "last_accrual_date"], path)
  return { outstanding: amount(parsed.outstanding, `${path}.outstanding`), accrued_unpaid: amount(parsed.accrued_unpaid, `${path}.accrued_unpaid`), carried: parseFraction(parsed.carried, `${path}.carried`), last_accrual_date: civilDate(parsed.last_accrual_date, `${path}.last_accrual_date`) }
}

function stringKey(key: string): void { string(key, key) }
