import { SaveSchemaError, array, exact, integer, record, string } from "../../primitives.ts"
import { amount, type DecimalAmount } from "../value.ts"

type EclScenario = { readonly weight_bp: number; readonly pd_bp: number; readonly lgd_bp: number }
type IndustrialFlowParams = { readonly customer: string; readonly supplier: string; readonly raw_item: string; readonly finished_item: string; readonly raw_account: string; readonly finished_account: string; readonly base_daily_demand_units: number; readonly unit_price_excl_vat: DecimalAmount; readonly receivable_credit_days: number; readonly raw_replenish_target_units: number; readonly raw_unit_cost_excl_vat: DecimalAmount; readonly daily_production_units: number; readonly daily_conversion_cost: DecimalAmount; readonly daily_admin_expense: DecimalAmount; readonly bad_debt_base_bp: number; readonly asset_impairment_fraction_bp: number }
type BankFlowParams = { readonly depositor: string; readonly borrower: string; readonly fee_customer: string; readonly deposit_principal: DecimalAmount; readonly deposit_rate_bp: number; readonly deposit_term_days: number; readonly deposit_every_days: number; readonly loan_principal: DecimalAmount; readonly loan_rate_bp: number; readonly loan_term_days: number; readonly lending_every_days: number; readonly daily_fee_income: DecimalAmount; readonly credit_deterioration_scenarios: readonly EclScenario[] }
type InsuranceFlowParams = { readonly policyholder: string; readonly daily_groups_base: number; readonly premium: DecimalAmount; readonly expected_claims: DecimalAmount; readonly risk_adjustment: DecimalAmount; readonly coverage_days: number; readonly claim_every_days: number; readonly claim_size: DecimalAmount }
type RealEstateFlowParams = { readonly land_seller: string; readonly contractor: string; readonly buyer: string; readonly project: string; readonly total_units: number; readonly land_cost: DecimalAmount; readonly development_days: number; readonly daily_development_spend: DecimalAmount; readonly presale_open_day: number; readonly presale_units_per_day: number; readonly unit_price: DecimalAmount; readonly delivery_lag_days: number }

export type FlowParams = { readonly Industrial: IndustrialFlowParams } | { readonly Bank: BankFlowParams } | { readonly Insurance: InsuranceFlowParams } | { readonly RealEstate: RealEstateFlowParams }

function variant(value: unknown, path: string): readonly [string, unknown] {
  const entries = Object.entries(record(value, path))
  if (entries.length !== 1 || entries[0] === undefined) throw new SaveSchemaError(path, "必须是单一行业参数变体")
  return entries[0]
}

function eclScenario(value: unknown, path: string): EclScenario {
  const item = record(value, path)
  exact(item, ["weight_bp", "pd_bp", "lgd_bp"], path)
  return { weight_bp: integer(item.weight_bp, `${path}.weight_bp`), pd_bp: integer(item.pd_bp, `${path}.pd_bp`), lgd_bp: integer(item.lgd_bp, `${path}.lgd_bp`) }
}

function industrial(value: unknown, path: string): IndustrialFlowParams {
  const item = record(value, path)
  exact(item, ["customer", "supplier", "raw_item", "finished_item", "raw_account", "finished_account", "base_daily_demand_units", "unit_price_excl_vat", "receivable_credit_days", "raw_replenish_target_units", "raw_unit_cost_excl_vat", "daily_production_units", "daily_conversion_cost", "daily_admin_expense", "bad_debt_base_bp", "asset_impairment_fraction_bp"], path)
  return { customer: string(item.customer, `${path}.customer`), supplier: string(item.supplier, `${path}.supplier`), raw_item: string(item.raw_item, `${path}.raw_item`), finished_item: string(item.finished_item, `${path}.finished_item`), raw_account: string(item.raw_account, `${path}.raw_account`), finished_account: string(item.finished_account, `${path}.finished_account`), base_daily_demand_units: integer(item.base_daily_demand_units, `${path}.base_daily_demand_units`), unit_price_excl_vat: amount(item.unit_price_excl_vat, `${path}.unit_price_excl_vat`), receivable_credit_days: integer(item.receivable_credit_days, `${path}.receivable_credit_days`), raw_replenish_target_units: integer(item.raw_replenish_target_units, `${path}.raw_replenish_target_units`), raw_unit_cost_excl_vat: amount(item.raw_unit_cost_excl_vat, `${path}.raw_unit_cost_excl_vat`), daily_production_units: integer(item.daily_production_units, `${path}.daily_production_units`), daily_conversion_cost: amount(item.daily_conversion_cost, `${path}.daily_conversion_cost`), daily_admin_expense: amount(item.daily_admin_expense, `${path}.daily_admin_expense`), bad_debt_base_bp: integer(item.bad_debt_base_bp, `${path}.bad_debt_base_bp`), asset_impairment_fraction_bp: integer(item.asset_impairment_fraction_bp, `${path}.asset_impairment_fraction_bp`) }
}

function bank(value: unknown, path: string): BankFlowParams {
  const item = record(value, path)
  exact(item, ["depositor", "borrower", "fee_customer", "deposit_principal", "deposit_rate_bp", "deposit_term_days", "deposit_every_days", "loan_principal", "loan_rate_bp", "loan_term_days", "lending_every_days", "daily_fee_income", "credit_deterioration_scenarios"], path)
  return { depositor: string(item.depositor, `${path}.depositor`), borrower: string(item.borrower, `${path}.borrower`), fee_customer: string(item.fee_customer, `${path}.fee_customer`), deposit_principal: amount(item.deposit_principal, `${path}.deposit_principal`), deposit_rate_bp: integer(item.deposit_rate_bp, `${path}.deposit_rate_bp`), deposit_term_days: integer(item.deposit_term_days, `${path}.deposit_term_days`), deposit_every_days: integer(item.deposit_every_days, `${path}.deposit_every_days`), loan_principal: amount(item.loan_principal, `${path}.loan_principal`), loan_rate_bp: integer(item.loan_rate_bp, `${path}.loan_rate_bp`), loan_term_days: integer(item.loan_term_days, `${path}.loan_term_days`), lending_every_days: integer(item.lending_every_days, `${path}.lending_every_days`), daily_fee_income: amount(item.daily_fee_income, `${path}.daily_fee_income`), credit_deterioration_scenarios: array(item.credit_deterioration_scenarios, `${path}.credit_deterioration_scenarios`).map((entry, index) => eclScenario(entry, `${path}.credit_deterioration_scenarios[${index}]`)) }
}

function insurance(value: unknown, path: string): InsuranceFlowParams {
  const item = record(value, path)
  exact(item, ["policyholder", "daily_groups_base", "premium", "expected_claims", "risk_adjustment", "coverage_days", "claim_every_days", "claim_size"], path)
  return { policyholder: string(item.policyholder, `${path}.policyholder`), daily_groups_base: integer(item.daily_groups_base, `${path}.daily_groups_base`), premium: amount(item.premium, `${path}.premium`), expected_claims: amount(item.expected_claims, `${path}.expected_claims`), risk_adjustment: amount(item.risk_adjustment, `${path}.risk_adjustment`), coverage_days: integer(item.coverage_days, `${path}.coverage_days`), claim_every_days: integer(item.claim_every_days, `${path}.claim_every_days`), claim_size: amount(item.claim_size, `${path}.claim_size`) }
}

function realEstate(value: unknown, path: string): RealEstateFlowParams {
  const item = record(value, path)
  exact(item, ["land_seller", "contractor", "buyer", "project", "total_units", "land_cost", "development_days", "daily_development_spend", "presale_open_day", "presale_units_per_day", "unit_price", "delivery_lag_days"], path)
  return { land_seller: string(item.land_seller, `${path}.land_seller`), contractor: string(item.contractor, `${path}.contractor`), buyer: string(item.buyer, `${path}.buyer`), project: string(item.project, `${path}.project`), total_units: integer(item.total_units, `${path}.total_units`), land_cost: amount(item.land_cost, `${path}.land_cost`), development_days: integer(item.development_days, `${path}.development_days`), daily_development_spend: amount(item.daily_development_spend, `${path}.daily_development_spend`), presale_open_day: integer(item.presale_open_day, `${path}.presale_open_day`), presale_units_per_day: integer(item.presale_units_per_day, `${path}.presale_units_per_day`), unit_price: amount(item.unit_price, `${path}.unit_price`), delivery_lag_days: integer(item.delivery_lag_days, `${path}.delivery_lag_days`) }
}

export function parseFlowParams(value: unknown, path: string): FlowParams {
  const [tag, body] = variant(value, path)
  switch (tag) {
    case "Industrial": return { Industrial: industrial(body, `${path}.Industrial`) }
    case "Bank": return { Bank: bank(body, `${path}.Bank`) }
    case "Insurance": return { Insurance: insurance(body, `${path}.Insurance`) }
    case "RealEstate": return { RealEstate: realEstate(body, `${path}.RealEstate`) }
    default: throw new SaveSchemaError(path, "包含无效行业参数变体")
  }
}
