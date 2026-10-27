import { SaveSchemaError, array, civilDate, exact, integer, map, record, string } from "../../primitives.ts"
import { parseBooks, type Books } from "../accounting/index.ts"
import { parseAccountingPeriod } from "../accounting/period.ts"
import { amount } from "../value.ts"
import { parseBudget, parseContractBook, parseCounterpartyLedger, parseFraction, parseI128Number, parseTradeOpenLedger, type ContractBook, type CounterpartyLedger, type OperatingBudget, type TradeOpenLedger } from "./common.ts"

export type IndustrialBooks = { readonly books: Books; readonly inventory: InventoryLedger; readonly assets: FixedAssetRegister; readonly receivables: TradeOpenLedger; readonly payables: TradeOpenLedger; readonly contracts: ContractBook; readonly counterparties: CounterpartyLedger; readonly budget: OperatingBudget; readonly tax_policy: TaxPolicy; readonly income_tax_position: IncomeTaxPosition; readonly loans: Readonly<Record<string, IndustrialLoanState>>; readonly inventory_source_events: readonly { readonly event: number; readonly item: string; readonly account: string }[]; readonly trade_counterparty_events: readonly { readonly event: number; readonly counterparty: string; readonly account: string }[]; readonly next_event_id: number }
type InventoryLedger = { readonly items: Readonly<Record<string, InventoryItemState>> }
type InventoryItemState = { readonly account: string; readonly quantity: number; readonly total_cost: string }
type FixedAssetRegister = { readonly assets: Readonly<Record<string, FixedAssetEntry>> }
type FixedAssetEntry = { readonly cost: string; readonly salvage_value: string; readonly life_months: number; readonly depreciated_months: number; readonly accumulated_depreciation: string; readonly accumulated_impairment: string }
type TaxPolicy = { readonly version: number; readonly vat: VatPolicy; readonly income_tax: IncomeTaxPolicy }
type VatPolicy = { readonly output_rate_bp: number; readonly input_rate_bp: number; readonly deductible_share_bp: number }
type IncomeTaxPolicy = { readonly rate_bp: number; readonly loss_carryforward_years: number }
type LossEntry = { readonly origin_year: number; readonly remaining: string }
type IncomeTaxPosition = { readonly loss_pool: readonly LossEntry[]; readonly assessments: Readonly<Record<string, IncomeTaxAssessment>>; readonly initial_deferred_tax_asset: string; readonly restatements: Readonly<Record<string, string>> }
type IncomeTaxAssessment = { readonly deferred_tax_asset: string; readonly pretax: string; readonly current_tax: string; readonly opening_loss_pool: readonly LossEntry[] }
type IndustrialLoanState = { readonly outstanding: string; readonly accrued_unpaid: string; readonly carried: string; readonly last_accrual_date: string }

export function parseIndustrialBooks(value: unknown, path: string): IndustrialBooks {
  const parsed = record(value, path)
  exact(parsed, ["books", "inventory", "assets", "receivables", "payables", "contracts", "counterparties", "budget", "tax_policy", "income_tax_position", "loans", "trade_counterparty_events", "inventory_source_events", "next_event_id"], path)
  const tradeEvents = array(parsed.trade_counterparty_events, `${path}.trade_counterparty_events`).map((value, index) => {
    const eventPath = `${path}.trade_counterparty_events[${index}]`
    const event = record(value, eventPath)
    exact(event, ["event", "counterparty", "account"], eventPath)
    return { event: integer(event.event, `${eventPath}.event`, 0), counterparty: string(event.counterparty, `${eventPath}.counterparty`), account: string(event.account, `${eventPath}.account`) }
  })
  const inventoryEvents = array(parsed.inventory_source_events, `${path}.inventory_source_events`).map((value, index) => {
    const eventPath = `${path}.inventory_source_events[${index}]`
    const event = record(value, eventPath)
    exact(event, ["event", "item", "account"], eventPath)
    return { event: integer(event.event, `${eventPath}.event`, 0), item: string(event.item, `${eventPath}.item`), account: string(event.account, `${eventPath}.account`) }
  })
  return { inventory_source_events: inventoryEvents, trade_counterparty_events: tradeEvents, books: parseBooks(parsed.books, `${path}.books`), inventory: parseInventory(parsed.inventory, `${path}.inventory`), assets: parseAssets(parsed.assets, `${path}.assets`), receivables: parseTradeOpenLedger(parsed.receivables, `${path}.receivables`), payables: parseTradeOpenLedger(parsed.payables, `${path}.payables`), contracts: parseContractBook(parsed.contracts, `${path}.contracts`), counterparties: parseCounterpartyLedger(parsed.counterparties, `${path}.counterparties`), budget: parseBudget(parsed.budget, `${path}.budget`), tax_policy: parseTaxPolicy(parsed.tax_policy, `${path}.tax_policy`), income_tax_position: parseIncomeTaxPosition(parsed.income_tax_position, `${path}.income_tax_position`), loans: map(parsed.loans, `${path}.loans`, stringKey, parseLoan), next_event_id: integer(parsed.next_event_id, `${path}.next_event_id`, 0) }
}

export function parseIncomeTaxPosition(value: unknown, path: string): IncomeTaxPosition {
  const parsed = record(value, path)
  exact(parsed, ["loss_pool", "assessments", "initial_deferred_tax_asset", "restatements"], path)
  const assessments = map(parsed.assessments, `${path}.assessments`, (key, keyPath) => {
    if (!/^(0|-?[1-9][0-9]*)$/.test(key) || BigInt(key) < -2_147_483_648n || BigInt(key) > 2_147_483_647n) throw new SaveSchemaError(keyPath, "必须是规范 i32 年度键")
  }, parseIncomeTaxAssessment)
  return { loss_pool: array(parsed.loss_pool, `${path}.loss_pool`).map((entry, index) => parseLossEntry(entry, `${path}.loss_pool[${index}]`)), assessments, initial_deferred_tax_asset: amount(parsed.initial_deferred_tax_asset, `${path}.initial_deferred_tax_asset`), restatements: map(parsed.restatements, `${path}.restatements`, (key, keyPath) => {
    if (!/^(0|[1-9][0-9]*)$/.test(key) || BigInt(key) > 18_446_744_073_709_551_615n) throw new SaveSchemaError(keyPath, "必须是规范 u64 BusinessEventId 键")
  }, parseAccountingPeriod) }
}

function parseIncomeTaxAssessment(value: unknown, path: string): IncomeTaxAssessment {
  const parsed = record(value, path)
  exact(parsed, ["pretax", "current_tax", "opening_loss_pool", "deferred_tax_asset"], path)
  return { deferred_tax_asset: amount(parsed.deferred_tax_asset, `${path}.deferred_tax_asset`), pretax: amount(parsed.pretax, `${path}.pretax`), current_tax: amount(parsed.current_tax, `${path}.current_tax`), opening_loss_pool: array(parsed.opening_loss_pool, `${path}.opening_loss_pool`).map((entry, index) => parseLossEntry(entry, `${path}.opening_loss_pool[${index}]`)) }
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

export function parseTaxPolicy(value: unknown, path: string): TaxPolicy {
  const parsed = record(value, path)
  exact(parsed, ["version", "vat", "income_tax"], path)
  return { version: integer(parsed.version, `${path}.version`, 0), vat: parseVat(parsed.vat, `${path}.vat`), income_tax: parseIncomeTax(parsed.income_tax, `${path}.income_tax`) }
}

function parseVat(value: unknown, path: string): VatPolicy {
  const parsed = record(value, path)
  exact(parsed, ["output_rate_bp", "input_rate_bp", "deductible_share_bp"], path)
  return { output_rate_bp: integer(parsed.output_rate_bp, `${path}.output_rate_bp`), input_rate_bp: integer(parsed.input_rate_bp, `${path}.input_rate_bp`), deductible_share_bp: integer(parsed.deductible_share_bp, `${path}.deductible_share_bp`) }
}

export function parseIncomeTax(value: unknown, path: string): IncomeTaxPolicy {
  const parsed = record(value, path)
  exact(parsed, ["rate_bp", "loss_carryforward_years"], path)
  const rate = integer(parsed.rate_bp, `${path}.rate_bp`, -2147483648)
  const years = integer(parsed.loss_carryforward_years, `${path}.loss_carryforward_years`, 0)
  if (rate > 2147483647) throw new SaveSchemaError(`${path}.rate_bp`, "超出 i32 范围")
  if (years > 65535) throw new SaveSchemaError(`${path}.loss_carryforward_years`, "超出 u16 范围")
  return { rate_bp: rate, loss_carryforward_years: years }
}

function parseLossEntry(value: unknown, path: string): LossEntry {
  const parsed = record(value, path)
  exact(parsed, ["origin_year", "remaining"], path)
  const year = integer(parsed.origin_year, `${path}.origin_year`, -2147483648)
  if (year > 2147483647) throw new SaveSchemaError(`${path}.origin_year`, "超出 i32 范围")
  return { origin_year: year, remaining: amount(parsed.remaining, `${path}.remaining`) }
}

function parseLoan(value: unknown, path: string): IndustrialLoanState {
  const parsed = record(value, path)
  exact(parsed, ["outstanding", "accrued_unpaid", "carried", "last_accrual_date"], path)
  return { outstanding: amount(parsed.outstanding, `${path}.outstanding`), accrued_unpaid: amount(parsed.accrued_unpaid, `${path}.accrued_unpaid`), carried: parseFraction(parsed.carried, `${path}.carried`), last_accrual_date: civilDate(parsed.last_accrual_date, `${path}.last_accrual_date`) }
}

function stringKey(key: string): void { string(key, key) }
