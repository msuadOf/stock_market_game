import assert from "node:assert/strict"
import test from "node:test"
import { matureLegacySaveFixture } from "./mature-save-test-fixture.ts"
import { parseIndustryBooks } from "./schema/company/books/index.ts"

const mature = matureLegacySaveFixture() as any
const industrial = mature.company_operations.companies["C-000812"].books
const books = { chart: { version: 1, accounts: {} }, journal: { batches: [], closed: [] } }
const counterparties = { counterparties: {}, flows: [] }
const bank = { Bank: { books, deposits: {}, loans: {}, counterparties, ecl_policy: { version: 1, stage1_default: [{ weight_bp: 10000, pd_bp: 100, lgd_bp: 4000 }], lifetime_default: [{ weight_bp: 10000, pd_bp: 800, lgd_bp: 6000 }] }, next_event_id: 2 } }
const insurance = { Insurance: { books, groups: {}, counterparties, discount: { version: 1, rate_bp: 400 }, next_event_id: 2 } }
const realEstate = { RealEstate: { books, projects: {}, presales: {}, loans: {}, receivables: { items: {}, written_off_total: "0.00" }, counterparties, budget: { operating_cash_floor: "0.00", credit_lines: {} }, capitalization_policy: { version: 1, suspension_min_days: 90 }, max_projects: 2, next_event_id: 2 } }

test("parseIndustryBooks preserves a populated Industrial slice", () => {
  assert.deepEqual(parseIndustryBooks(industrial, "save.company_operations.companies.C-000812.books"), industrial)
})

test("parseIndustryBooks rejects malformed industrial inventory structures with paths", () => {
  assert.throws(() => parseIndustryBooks({ Industrial: { ...industrial.Industrial, inventory: 7 } }, "save.books"), /save\.books\.Industrial\.inventory/)
  assert.throws(() => parseIndustryBooks({ Industrial: { ...industrial.Industrial, inventory: {} } }, "save.books"), /save\.books\.Industrial\.inventory\.items/)
})

test("parseIndustryBooks preserves source-shaped empty Bank Insurance and RealEstate books", () => {
  assert.deepEqual(parseIndustryBooks(bank, "save.books"), bank)
  assert.deepEqual(parseIndustryBooks(insurance, "save.books"), insurance)
  assert.deepEqual(parseIndustryBooks(realEstate, "save.books"), realEstate)
})

test("parseIndustryBooks rejects malformed exact nested fields and wire scalars", () => {
  assert.throws(() => parseIndustryBooks({ Bank: { ...bank.Bank, next_event_id: "2" } }, "save.books"), /save\.books\.Bank\.next_event_id/)
  assert.throws(() => parseIndustryBooks({ Bank: { ...bank.Bank, ecl_policy: { ...bank.Bank.ecl_policy, lifetime_default: [{ weight_bp: 10000, pd_bp: 800, lgd_bp: 6000, extra: true }] } } }, "save.books"), /save\.books\.Bank\.ecl_policy\.lifetime_default\[0\]\.extra/)
  assert.throws(() => parseIndustryBooks({ Insurance: { ...insurance.Insurance, discount: { ...insurance.Insurance.discount, rate_bp: "400" } } }, "save.books"), /save\.books\.Insurance\.discount\.rate_bp/)
  assert.throws(() => parseIndustryBooks({ RealEstate: { ...realEstate.RealEstate, extra: true } }, "save.books"), /save\.books\.RealEstate\.extra/)
  assert.throws(() => parseIndustryBooks({ Industrial: { ...industrial.Industrial, inventory: { items: { RAW: { account: "1403", quantity: Number.MAX_SAFE_INTEGER + 1, total_cost: "1.00" } } } } }, "save.books"), /save\.books\.Industrial\.inventory\.items\.RAW\.quantity/)
  assert.throws(() => parseIndustryBooks({ Bank: { ...bank.Bank, ecl_policy: { ...bank.Bank.ecl_policy, stage1_default: [{ weight_bp: 10000, pd_bp: 100, lgd_bp: "4000" }] } } }, "save.books"), /save\.books\.Bank\.ecl_policy\.stage1_default\[0\]\.lgd_bp/)
  assert.throws(() => parseIndustryBooks({ RealEstate: { ...realEstate.RealEstate, receivables: { items: {}, written_off_total: "1.234" } } }, "save.books"), /save\.books\.RealEstate\.receivables\.written_off_total/)
  assert.throws(() => parseIndustryBooks({ Insurance: { ...insurance.Insurance, discount: { version: 1, rate_bp: 400 }, bad: false } }, "save.books"), /save\.books\.Insurance\.bad/)
  assert.throws(() => parseIndustryBooks({ Unknown: {} }, "save.books"), /save\.books/)
})

test("parseIndustryBooks accepts fully populated source-shaped subledgers", () => {
  const populatedBank = { Bank: { ...bank.Bank, deposits: { "DEP-1": { principal: "100.00", accrued_payable: "1.00", carried: "0", last_accrual_date: "2030-01-01", rate_bp: 150, counterparty: "EXT-DEP", start_date: "2030-01-01", maturity_date: "2030-01-11" } }, loans: { "LN-1": { principal: "80.00", accrued_receivable: "1.00", carried: "0", last_accrual_date: "2030-01-01", rate_bp: 400, counterparty: "EXT-BOR", stage: "Stage2", allowance: "4.00", written_off: false, recoverable: "0.00", transfers: [{ date: "2030-01-02", from_stage: "Stage1", to_stage: "Stage2", reason: "fixture" }] } } } }
  const populatedInsurance = { Insurance: { ...insurance.Insurance, groups: { "GRP-1": { policyholder: "EXT-POL", premium: "60.00", premium_collected: "50.00", expected_claims_remaining: "40.00", risk_adjustment_remaining: "3.00", csm: "7.00", loss_component: "0.00", finance_remaining: "1.00", units_total: 30, units_released: 5, coverage_start: "2030-01-01", coverage_end: "2030-01-31", day_one_loss: "0.00", released_revenue: "10.00", released_finance: "0.00", remeasure_finance: "0.00", remeasure_loss: "0.00", reestimated_csm: "0.00", carried_claims: "0", carried_risk_adjustment: "0", carried_csm: "0", carried_finance: "0", carried_loss: "0", claims: { "CLM-1": { incurred: "10.00", paid: "4.00", date_incurred: "2030-01-02" } } } } } }
  const populatedRealEstate = { RealEstate: { ...realEstate.RealEstate, projects: { "P-1": { total_units: 8, remaining_units: 6, land_cost: "2000.00", development_cost: "500.00", capitalized_interest: "10.00", remaining_cost: "1882.50", carried_out_cost: "627.50", dev_started_on: "2030-01-02", interrupted_on: null, interruptions: [{ start: "2030-01-03", end: "2030-01-04" }], completed_on: null } }, presales: { "PS-1": { project: "P-1", buyer: "EXT-BUYER", units: 2, price_total: "600.00", collected: "300.00", delivered: false } }, loans: { "LN-1": { outstanding: "500.00", accrued_unpaid: "1.00", carried_cap: "0", carried_exp: "0", last_accrual_date: "2030-01-02", lender: "EXT-LEND", project: "P-1", debt_account: "2001", annual_rate_bp: 365 } } } }
  assert.deepEqual(parseIndustryBooks(populatedBank, "save.books"), populatedBank)
  assert.deepEqual(parseIndustryBooks(populatedInsurance, "save.books"), populatedInsurance)
  assert.deepEqual(parseIndustryBooks(populatedRealEstate, "save.books"), populatedRealEstate)
})
