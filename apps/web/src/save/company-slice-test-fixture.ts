import { owners } from "./schema/company/books/tax-owner-test-fixture.ts"

const industrial = structuredClone(owners.Industrial)
const books = {
  chart: { version: 1, accounts: { ...industrial.books.chart.accounts, "1002": { name: "银行存款", element: "Asset", is_cash: true, is_contra: false }, "4001": { name: "实收资本", element: "Equity", is_cash: false, is_contra: false } } },
  journal: { batches: [[{ source: 1, date: "2030-01-01", kind: "OpeningBalance", cash_flow: "NonCash", lines: [{ account: "1002", side: "Debit", amount: "100.00" }, { account: "4001", side: "Credit", amount: "100.00" }] }]], closed: [] },
}
const operations = {
  seed: "42",
  shock_params: { version: 1, market_candidate_bp: 100, industry_candidate_bp: 100, company_candidate_bp: 100, duration_min_days: 1, duration_max_days: 2, market_demand_band_bp: 50, industry_cost_band_bp: 50, company_demand_band_bp: 50, credit_deterioration_add_bp: 0 },
  scheduler: { next_seq: 1, settled_through: "2030-01-01", pending: [{ id: 0, key: "interest", due_date: "2030-01-02", action: { InterestAccrual: { company: "C-000812" } } }] },
  market_rng: { state: "42" }, industry_rngs: { industrial: { state: "43" } },
  companies: { "C-000812": {
    spec: { id: "C-000812", name: "虚拟低层公司", industry: "industrial", kind: "Industrial", listed_stock: "000812", issued_shares: "1000", group_parent: null },
    books: { Industrial: { ...industrial, books, next_event_id: 2 } },
    params: { Industrial: { customer: "CUST", supplier: "SUPP", raw_item: "RAW", finished_item: "FIN", raw_account: "1403", finished_account: "1405", base_daily_demand_units: 10, unit_price_excl_vat: "100.00", receivable_credit_days: 5, raw_replenish_target_units: 20, raw_unit_cost_excl_vat: "40.00", daily_production_units: 8, daily_conversion_cost: "30.00", daily_admin_expense: "10.00", bad_debt_base_bp: 100, asset_impairment_fraction_bp: 2000 } },
    rng: { state: "44" }, economy: { active: [{ kind: "CompanyDemandShift", amplitude_bp: 10, starts_on: "2030-01-01", expires_on: "2030-01-02" }] }, next_flow_seq: 1,
  } },
  next_expected: "2030-01-02", history: { generated_through: "2030-01-01" }, payment_failures: {},
}

export function representativeCompanyOperationsFixture() {
  return structuredClone(operations)
}
