import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import test from "node:test"
import { parseSaveSlot } from "./save-schema.ts"

const maturePath = "/home/baiyifan/.claude/tmp/opencode/task29-save.json"

function matureSave(): unknown {
  return JSON.parse(readFileSync(maturePath, "utf8"))
}

function mutate(path: readonly (string | number)[], value: unknown): unknown {
  const save = structuredClone(matureSave())
  let cursor: unknown = save
  for (const segment of path.slice(0, -1)) {
    if (typeof segment === "number") {
      assert.ok(Array.isArray(cursor))
      cursor = cursor[segment]
    } else {
      assert.ok(typeof cursor === "object" && cursor !== null && !Array.isArray(cursor))
      cursor = Reflect.get(cursor, segment)
    }
  }
  const last = path.at(-1)
  if (last === undefined) throw new Error("mutation path must not be empty")
  assert.ok(typeof cursor === "object" && cursor !== null)
  Reflect.set(cursor, last, value)
  return save
}

test("strict save boundary preserves the mature Rust save", () => {
  const save = matureSave()
  assert.deepEqual(parseSaveSlot(save), save)
})

test("strict save boundary rejects malformed nested K7 branches", () => {
  const save = matureSave()
  assert.ok(typeof save === "object" && save !== null)
  const operations = Reflect.get(save, "company_operations")
  assert.ok(typeof operations === "object" && operations !== null)
  const companies = Reflect.get(operations, "companies")
  assert.ok(typeof companies === "object" && companies !== null)
  const company = Object.keys(companies)[0]
  const information = Reflect.get(save, "information_states")
  assert.ok(typeof information === "object" && information !== null)
  const account = Object.keys(information)[0]
  const history = Reflect.get(save, "price_history")
  assert.ok(typeof history === "object" && history !== null)
  const market = Object.keys(history)[0]
  const planBook = Reflect.get(save, "plans")
  assert.ok(typeof planBook === "object" && planBook !== null)
  const plans = Reflect.get(planBook, "plans")
  assert.ok(typeof plans === "object" && plans !== null)
  const plan = Object.keys(plans)[0]
  if (company === undefined || account === undefined) throw new Error("mature save must contain K7 company and personal state")
  if (market === undefined || plan === undefined) throw new Error("mature save must contain market and plan state")

  const cases: readonly [readonly (string | number)[], unknown, RegExp][] = [
    [["setup", "start_date"], "2030-02-30", /setup\.start_date/],
    [["snapshot", "seq"], "0", /snapshot\.seq/],
    [["auction_orders", "600101"], [{ owner: 1 }], /auction_orders/],
    [["resting_orders", market], [{ id: 1 }], /resting_orders/],
    [["price_history", market, 0], 1.5, /price_history/],
    [["market_minute_closes", market], [{ absolute_trading_minute: 1, close: "1000" }], /market_minute_closes/],
    [["rng_state"], 42, /rng_state/],
    [["npc_attention", account, "rng_state"], 42, /npc_attention/],
    [["strategy_profiles", account], { Institution: "Unknown" }, /strategy_profiles/],
    [["retail_experience", "1", "consecutive_failed_buys"], "1", /retail_experience/],
    [["parent_orders"], { [account]: { [market]: { code: market } } }, /parent_orders/],
    [["npc_order_lifecycles"], [{ account: 1 }], /npc_order_lifecycles/],
    [["pending_player"], [[1, { Retired: {} }]], /pending_player/],
    [["next_order_id"], 9_007_199_254_740_992, /next_order_id/],
    [["civil_clock", "policy", "unexpected"], true, /civil_clock\.policy\.unexpected/],
    [["company_operations", "companies", company, "spec", "unexpected"], true, /company_operations/],
    [["closing_registry", "versions"], [{}], /closing_registry/],
    [["public_library", "next_seq"], "35", /public_library/],
    [["ops_wiring", "mirrored", 0], "7067", /ops_wiring/],
    [["disclosures", "announced_through"], "2030-02-30", /disclosures/],
    [["plans", "plans", plan, "status"], { Unknown: {} }, /plans/],
    [["information_states", account, "unexpected"], true, /information_states/],
    [["belief_books", account, "analysis", "fundamental_bp"], "4000", /belief_books/],
    [["watchlists", account, "latest_attention_minute"], 50, /watchlists/],
    [["price_memories", account, "stocks", market, "last_touched_minute"], 50, /price_memories/],
    [["pending_plan_events"], [{ Rejected: {} }], /pending_plan_events/],
  ]
  for (const [path, value, expected] of cases) assert.throws(() => parseSaveSlot(mutate(path, value)), expected)
})

test("strict save boundary rejects unsafe company RNG numbers", () => {
  assert.throws(
    () => parseSaveSlot(mutate(["company_operations", "market_rng", "state"], 9_007_199_254_740_992)),
    /company_operations\.market_rng\.state/,
  )
})

test("strict save boundary rejects malformed company book and flow internals", () => {
  const save = matureSave()
  assert.ok(typeof save === "object" && save !== null)
  const operations = Reflect.get(save, "company_operations")
  assert.ok(typeof operations === "object" && operations !== null)
  const companies = Reflect.get(operations, "companies")
  assert.ok(typeof companies === "object" && companies !== null)
  const company = Object.keys(companies)[0]
  if (company === undefined) throw new Error("mature save must contain a company")

  const cases: readonly [readonly (string | number)[], unknown, RegExp][] = [
    [["company_operations", "companies", company, "books", "Industrial", "inventory"], 7, /company_operations\.companies\..*\.books\.Industrial\.inventory/],
    [["company_operations", "companies", company, "books", "Industrial", "books", "chart", "accounts", "1001", "name"], 7, /company_operations\.companies\..*\.books\.Industrial\.books\.chart\.accounts\.1001\.name/],
    [["company_operations", "companies", company, "params", "Industrial", "unit_price_excl_vat"], {}, /company_operations\.companies\..*\.params\.Industrial\.unit_price_excl_vat/],
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
    const save = structuredClone(matureSave())
    assert.ok(typeof save === "object" && save !== null)
    const operations = Reflect.get(save, "company_operations")
    assert.ok(typeof operations === "object" && operations !== null)
    const companies = Reflect.get(operations, "companies")
    assert.ok(typeof companies === "object" && companies !== null)
    const company = Object.keys(companies)[0]
    if (company === undefined) throw new Error("mature save must contain a company")
    const operatingCompany = Reflect.get(companies, company)
    assert.ok(typeof operatingCompany === "object" && operatingCompany !== null)
    Reflect.set(operatingCompany, "params", flow)
    assert.throws(() => parseSaveSlot(save), expected)
  }
})
