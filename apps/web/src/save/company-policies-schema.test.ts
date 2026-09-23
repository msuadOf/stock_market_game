import assert from "node:assert/strict"
import test from "node:test"
import { matureLegacySaveFixture } from "./mature-save-test-fixture.ts"
import { record } from "./schema/primitives.ts"
import { parseFlowParams } from "./schema/company/policies/flow.ts"
import { parseHistory } from "./schema/company/policies/history.ts"
import { parseScheduler } from "./schema/company/policies/scheduler.ts"
import { parseActiveShock, parseShockParams } from "./schema/company/policies/shock.ts"

function matureOperations(): Record<string, unknown> {
  const save = matureLegacySaveFixture()
  return record(record(save, "save").company_operations, "company_operations")
}

const industrial = {
  Industrial: {
    customer: "CUST", supplier: "SUPP", raw_item: "RAW", finished_item: "FIN", raw_account: "1403", finished_account: "1405", base_daily_demand_units: 10, unit_price_excl_vat: "100.00", receivable_credit_days: 5, raw_replenish_target_units: 20, raw_unit_cost_excl_vat: "40.00", daily_production_units: 8, daily_conversion_cost: "30.00", daily_admin_expense: "10.00", bad_debt_base_bp: 100, asset_impairment_fraction_bp: 2000,
  },
}
const bank = { Bank: { depositor: "DEP", borrower: "BOR", fee_customer: "FEE", deposit_principal: "1000.00", deposit_rate_bp: 150, deposit_term_days: 10, deposit_every_days: 3, loan_principal: "800.00", loan_rate_bp: 400, loan_term_days: 5, lending_every_days: 3, daily_fee_income: "5.00", credit_deterioration_scenarios: [{ weight_bp: 10000, pd_bp: 2000, lgd_bp: 5000 }] } }
const insurance = { Insurance: { policyholder: "POL", daily_groups_base: 2, premium: "60.00", expected_claims: "50.00", risk_adjustment: "3.00", coverage_days: 30, claim_every_days: 10, claim_size: "10.00" } }
const realEstate = { RealEstate: { land_seller: "LAND", contractor: "CON", buyer: "BUY", project: "P-1", total_units: 8, land_cost: "2000.00", development_days: 6, daily_development_spend: "500.00", presale_open_day: 1, presale_units_per_day: 2, unit_price: "300.00", delivery_lag_days: 2 } }

test("company policy parsers preserve mature and every flow variant", () => {
  const operations = matureOperations()
  const companies = record(operations.companies, "company_operations.companies")
  const first = Object.values(companies)[0]
  if (first === undefined) throw new Error("mature save must have a company")
  const company = record(first, "company")
  assert.deepEqual(parseFlowParams(company.params, "company.params"), company.params)
  assert.deepEqual(parseShockParams(operations.shock_params, "company_operations.shock_params"), operations.shock_params)
  assert.deepEqual(parseScheduler(operations.scheduler, "company_operations.scheduler"), operations.scheduler)
  assert.deepEqual(parseHistory(operations.history, "company_operations.history"), operations.history)
  assert.deepEqual(parseFlowParams(industrial, "flow"), industrial)
  assert.deepEqual(parseFlowParams(bank, "flow"), bank)
  assert.deepEqual(parseFlowParams(insurance, "flow"), insurance)
  assert.deepEqual(parseFlowParams(realEstate, "flow"), realEstate)
  const shock = { kind: { IndustryCostShift: { industry: "home-appliances" } }, amplitude_bp: -50, starts_on: "2030-01-01", expires_on: "2030-01-02" }
  assert.deepEqual(parseActiveShock(shock, "shock"), shock)
})

test("company policy parsers reject malformed exact values", () => {
  assert.throws(() => parseFlowParams({ Unknown: {} }, "flow"), /flow/)
  assert.throws(() => parseFlowParams({ Industrial: { ...industrial.Industrial, base_daily_demand_units: "10" } }, "flow"), /base_daily_demand_units/)
  assert.throws(() => parseScheduler({ next_seq: 9_007_199_254_740_992, settled_through: null, pending: [] }, "scheduler"), /next_seq/)
  assert.throws(() => parseActiveShock({ kind: "MarketDemandShift", amplitude_bp: 1, starts_on: "2030-02-30", expires_on: "2030-03-01" }, "shock"), /starts_on/)
  assert.throws(() => parseScheduler({ next_seq: 1, settled_through: null, pending: [{ id: 0, key: "DUE", due_date: "2030-01-02", action: { ContractMaturity: { company: "C" } } }] }, "scheduler"), /reference/)
  assert.throws(() => parseScheduler({ next_seq: 1, settled_through: null, pending: [{ id: 0, key: "DUE", due_date: "2030-01-02", action: { InterestAccrual: { company: 1 } } }] }, "scheduler"), /company/)
  assert.throws(() => parseActiveShock({ kind: "MarketDemandShift", amplitude_bp: 1, starts_on: "2030-01-01", expires_on: "2030-01-02", extra: true }, "shock"), /extra/)
  assert.throws(() => parseActiveShock({ kind: "MarketDemandShift", amplitude_bp: 1, starts_on: "2030-01-03", expires_on: "2030-01-02" }, "shock"), /expires_on/)
  assert.throws(() => parseShockParams({ ...record(matureOperations().shock_params, "shock_params"), market_candidate_bp: 10_001 }, "shock_params"), /shock_params/)
  assert.throws(() => parseHistory({ generated_through: 1 }, "history"), /generated_through/)
})
