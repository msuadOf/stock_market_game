import assert from "node:assert/strict"
import test from "node:test"
import { matureSave, mutate } from "./complete-save-test-helper.ts"
import { parseSaveSlot } from "./save-schema.ts"

test("strict save boundary preserves a populated current-schema save", () => {
  const save = matureSave()
  assert.deepEqual(parseSaveSlot(save), save)
})

test("strict save boundary rejects unsafe company RNG numbers", () => {
  assert.throws(
    () => parseSaveSlot(mutate(["company_state", "LegacyOperations", "operations", "market_rng", "state"], 9_007_199_254_740_992)),
    /company_state\.LegacyOperations\.operations\.market_rng\.state/,
  )
})

test("strict save boundary rejects malformed company book and flow internals", () => {
  const save = matureSave()
  assert.ok(typeof save === "object" && save !== null)
  const operations = Reflect.get(Reflect.get(Reflect.get(save, "company_state"), "LegacyOperations"), "operations")
  assert.ok(typeof operations === "object" && operations !== null)
  const companies = Reflect.get(operations, "companies")
  assert.ok(typeof companies === "object" && companies !== null)
  const company = Object.keys(companies)[0]
  if (company === undefined) throw new Error("mature save must contain a company")

  const cases: readonly [readonly (string | number)[], unknown, RegExp][] = [
    [["company_state", "LegacyOperations", "operations", "companies", company, "books", "Industrial", "inventory"], 7, /company_state\.LegacyOperations\.operations\.companies\..*\.books\.Industrial\.inventory/],
    [["company_state", "LegacyOperations", "operations", "companies", company, "books", "Industrial", "books", "chart", "accounts", "1001", "name"], 7, /company_state\.LegacyOperations\.operations\.companies\..*\.books\.Industrial\.books\.chart\.accounts\.1001\.name/],
    [["company_state", "LegacyOperations", "operations", "companies", company, "params", "Industrial", "unit_price_excl_vat"], {}, /company_state\.LegacyOperations\.operations\.companies\..*\.params\.Industrial\.unit_price_excl_vat/],
  ]
  for (const [path, value, expected] of cases) assert.throws(() => parseSaveSlot(mutate(path, value)), expected)
})

test("strict save boundary rejects malformed representative fields for every flow variant", () => {
  const flows: readonly [unknown, RegExp][] = [
    [{ Industrial: { customer: "C", supplier: "S", raw_item: "RAW", finished_item: "FIN", raw_account: "1403", finished_account: "1405", base_daily_demand_units: 1, unit_price_excl_vat: {}, receivable_credit_days: 1, raw_replenish_target_units: 1, raw_unit_cost_excl_vat: "1.00", daily_production_units: 1, daily_conversion_cost: "1.00", daily_admin_expense: "1.00", bad_debt_base_bp: 1, asset_impairment_fraction_bp: 1 } }, /params\.Industrial\.unit_price_excl_vat/],
    [{ Bank: { depositor: "D", borrower: "B", fee_customer: "F", deposit_principal: {}, deposit_rate_bp: 1, deposit_term_days: 1, deposit_every_days: 1, loan_principal: "1.00", loan_rate_bp: 1, loan_term_days: 1, lending_every_days: 1, daily_fee_income: "1.00", credit_deterioration_scenarios: [] } }, /params\.Bank\.deposit_principal/],
    [{ Insurance: { policyholder: "P", daily_groups_base: 1, premium: "1.00", expected_claims: {}, risk_adjustment: "1.00", coverage_days: 1, claim_every_days: 1, claim_size: "1.00" } }, /params\.Insurance\.expected_claims/],
    [{ RealEstate: { land_seller: "L", contractor: "C", buyer: "B", project: "P", total_units: 1, land_cost: "1.00", development_days: 1, daily_development_spend: "1.00", presale_open_day: 1, presale_units_per_day: 1, unit_price: {}, delivery_lag_days: 1 } }, /params\.RealEstate\.unit_price/],
  ]
  for (const [flow, expected] of flows) {
    const save = matureSave()
    assert.ok(typeof save === "object" && save !== null)
    const operations = Reflect.get(Reflect.get(Reflect.get(save, "company_state"), "LegacyOperations"), "operations")
    assert.ok(typeof operations === "object" && operations !== null)
    const companies = Reflect.get(operations, "companies")
    assert.ok(typeof companies === "object" && companies !== null)
    const company = Object.keys(companies)[0]
    if (company === undefined) throw new Error("mature save must contain a company")
    assert.throws(() => parseSaveSlot(mutate(["company_state", "LegacyOperations", "operations", "companies", company, "params"], flow)), expected)
  }
})
