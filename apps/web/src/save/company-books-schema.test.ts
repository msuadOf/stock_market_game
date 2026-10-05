import assert from "node:assert/strict"
import test from "node:test"
import { representativeCompanyOperationsFixture } from "./company-slice-test-fixture.ts"
import { parseIndustryBooks } from "./schema/company/books/index.ts"
import { policy, position, books, owners } from "./schema/company/books/tax-owner-test-fixture.ts"
import { bankLoanSourceFixture } from "./schema/company/books/bank-loan-source-test-fixture.ts"

const industrial = (representativeCompanyOperationsFixture() as any).companies["C-000812"].books
const counterparties = { counterparties: {}, flows: [] }
const bank = { Bank: { income_tax_policy: policy, income_tax_position: position, books, deposits: {}, loans: {}, loan_claim_sources: [], counterparties, ecl_policy: { version: 1, stage1_default: [{ weight_bp: 10000, pd_bp: 100, lgd_bp: 4000 }], lifetime_default: [{ weight_bp: 10000, pd_bp: 800, lgd_bp: 6000 }] }, next_event_id: 2 } }
const insurance = { Insurance: { income_tax_policy: policy, income_tax_position: position, books, groups: {}, counterparties, discount: { version: 1, rate_bp: 400 }, next_event_id: 2 } }
const realEstate = { RealEstate: { income_tax_policy: policy, income_tax_position: position, books, projects: {}, presales: {}, loans: {}, receivables: { items: {}, written_off_total: "0.00" }, counterparties, budget: { operating_cash_floor: "0.00", credit_lines: {} }, capitalization_policy: { version: 1, suspension_min_days: 90 }, max_projects: 2, next_event_id: 2 } }

test("RealEstate 贷款保留到期日，允许逾期并拒绝缺失或非法日期", () => {
  const loan = { outstanding: "1.00", accrued_unpaid: "0.00", carried_cap: "0", carried_exp: "0", last_accrual_date: "2030-01-02", maturity_date: "2030-01-01", lender: "EXT", project: null, debt_account: "2001", annual_rate_bp: 365 }
  const saved = { RealEstate: { ...realEstate.RealEstate, loans: { LOAN: loan } } }
  assert.deepEqual(parseIndustryBooks(saved, "books"), saved)
  const { maturity_date: _removed, ...missing } = loan
  for (const invalid of [missing, { ...loan, maturity_date: "2030-02-30" }, { ...loan, maturity_date: null }]) assert.throws(() => parseIndustryBooks({ RealEstate: { ...realEstate.RealEstate, loans: { LOAN: invalid } } }, "books"), /maturity_date/)
})

test("parseIndustryBooks preserves a populated Industrial slice", () => {
  assert.deepEqual(parseIndustryBooks(industrial, "save.company_operations.companies.C-000812.books"), industrial)
})

test("IndustrialBooks 严格保存交易归属身份而非派生金额", () => {
  const events = [{ event: 1, counterparty: "PARTY", account: "1122" }]
  const saved = { Industrial: { ...industrial.Industrial, trade_counterparty_events: events } }
  assert.deepEqual(parseIndustryBooks(saved, "books"), saved)
  const { trade_counterparty_events: _removed, ...missing } = saved.Industrial
  assert.throws(() => parseIndustryBooks({ Industrial: missing }, "books"), /trade_counterparty_events/)
  for (const invalid of [{ ...events[0], event: "1" }, { ...events[0], amount: "1.00" }]) assert.throws(() => parseIndustryBooks({ Industrial: { ...saved.Industrial, trade_counterparty_events: [invalid] } }, "books"), /trade_counterparty_events/)
})

test("IndustrialBooks 严格保存库存来源身份而非派生金额或数量", () => {
  const events = [{ event: 1, item: "RAW", account: "1403" }]
  const saved = { Industrial: { ...industrial.Industrial, inventory_source_events: events } }
  assert.deepEqual(parseIndustryBooks(saved, "books"), saved)
  const { inventory_source_events: _removed, ...missing } = saved.Industrial
  assert.throws(() => parseIndustryBooks({ Industrial: missing }, "books"), /inventory_source_events/)
  for (const invalid of [{ ...events[0], event: "1" }, { ...events[0], amount: "1.00" }, { ...events[0], quantity: 1 }]) assert.throws(() => parseIndustryBooks({ Industrial: { ...saved.Industrial, inventory_source_events: [invalid] } }, "books"), /inventory_source_events/)
})

test("IndustrialBooks 所得税年度基准必须完整且拒绝旧亏损池结构", () => {
  const position = {
    loss_pool: [],
    assessments: { "2030": { pretax: "3.00", current_tax: "0.25", opening_loss_pool: [{ origin_year: 2029, remaining: "2.00" }], deferred_tax_asset: "0.00" } },
    initial_deferred_tax_asset: "0.00", restatements: {},
  }
  const accrual = { source: 1, date: "2030-12-31", kind: "TaxAccrual", cash_flow: "NonCash", lines: [{ account: "6801", side: "Debit", amount: "0.25" }, { account: "222104", side: "Credit", amount: "0.25" }] }
  const saved = { Industrial: { ...owners.Industrial, income_tax_position: position, books: { ...books, journal: { batches: [[accrual]], closed: [] } }, next_event_id: 2 } }
  const { income_tax_position: _currentPosition, ...base } = saved.Industrial
  assert.deepEqual(parseIndustryBooks(saved, "books"), saved)
  assert.throws(() => parseIndustryBooks({ Industrial: { ...base, loss_pool: [] } }, "books"), /income_tax_position/)
  for (const invalid of [[], { loss_pool: [] }, { ...position, extra: true }, { ...position, assessments: { "2030": { ...position.assessments["2030"], current_tax: 25 } } }, { ...position, assessments: { "02030": position.assessments["2030"] } }, { ...position, restatements: { "9007199254740992": "2030-01" } }]) {
    assert.throws(() => parseIndustryBooks({ Industrial: { ...base, income_tax_position: invalid } }, "books"), /income_tax_position/)
  }
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
  const populatedBank = { Bank: { ...bankLoanSourceFixture(), deposits: { "DEP-1": { principal: "100.00", accrued_payable: "1.00", carried: "0", last_accrual_date: "2030-01-01", rate_bp: 150, counterparty: "EXT-DEP", start_date: "2030-01-01", maturity_date: "2030-01-11" } } } }
  const populatedInsurance = { Insurance: { ...insurance.Insurance, groups: { "GRP-1": { policyholder: "EXT-POL", premium: "60.00", premium_collected: "50.00", expected_claims_remaining: "40.00", risk_adjustment_remaining: "3.00", csm: "7.00", loss_component: "0.00", finance_remaining: "1.00", units_total: 30, units_released: 5, coverage_start: "2030-01-01", coverage_end: "2030-01-31", day_one_loss: "0.00", released_revenue: "10.00", released_finance: "0.00", remeasure_finance: "0.00", remeasure_loss: "0.00", reestimated_csm: "0.00", carried_claims: "0", carried_risk_adjustment: "0", carried_csm: "0", carried_finance: "0", carried_loss: "0", claims: { "CLM-1": { incurred: "10.00", paid: "4.00", date_incurred: "2030-01-02" } } } } } }
  const populatedRealEstate = { RealEstate: { ...realEstate.RealEstate, projects: { "P-1": { total_units: 8, remaining_units: 6, land_cost: "2000.00", development_cost: "500.00", capitalized_interest: "10.00", remaining_cost: "1882.50", carried_out_cost: "627.50", dev_started_on: "2030-01-02", interrupted_on: null, interruptions: [{ start: "2030-01-03", end: "2030-01-04" }], completed_on: null } }, presales: { "PS-1": { project: "P-1", buyer: "EXT-BUYER", units: 2, price_total: "600.00", collected: "300.00", delivered: false } }, loans: { "LN-1": { outstanding: "500.00", accrued_unpaid: "1.00", carried_cap: "0", carried_exp: "0", last_accrual_date: "2030-01-02", maturity_date: "2030-01-01", lender: "EXT-LEND", project: "P-1", debt_account: "2001", annual_rate_bp: 365 } } } }
  assert.deepEqual(parseIndustryBooks(populatedBank, "save.books"), populatedBank)
  assert.deepEqual(parseIndustryBooks(populatedInsurance, "save.books"), populatedInsurance)
  assert.deepEqual(parseIndustryBooks(populatedRealEstate, "save.books"), populatedRealEstate)
})
