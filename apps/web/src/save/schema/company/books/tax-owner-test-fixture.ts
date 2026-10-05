export const policy = { rate_bp: 2500, loss_carryforward_years: 5 }
export const position = { loss_pool: [], assessments: {}, initial_deferred_tax_asset: "0.00", restatements: {} }
export const definitions = { "1811": { name: "递延所得税资产", element: "Asset", is_cash: false, is_contra: false }, "222104": { name: "应交所得税", element: "Liability", is_cash: false, is_contra: false }, "6801": { name: "所得税费用", element: "Expense", is_cash: false, is_contra: false }, "1001": { name: "现金", element: "Asset", is_cash: true, is_contra: false } }
export const books = { chart: { version: 1, accounts: definitions }, journal: { batches: [], closed: [] } }
export const counterparties = { counterparties: {}, flows: [] }
export const owners = {
  Industrial: { books, inventory: { items: {} }, assets: { assets: {} }, receivables: { items: {}, written_off_total: "0.00" }, payables: { items: {}, written_off_total: "0.00" }, contracts: { contracts: {} }, counterparties, budget: { operating_cash_floor: "0.00", credit_lines: {} }, tax_policy: { version: 1, vat: { output_rate_bp: 0, input_rate_bp: 0, deductible_share_bp: 0 }, income_tax: policy }, income_tax_position: position, loans: {}, inventory_source_events: [], trade_counterparty_events: [], next_event_id: 1 },
  Bank: { books, deposits: {}, loans: {}, loan_claim_sources: [], counterparties, ecl_policy: { version: 1, stage1_default: [], lifetime_default: [] }, income_tax_policy: policy, income_tax_position: position, next_event_id: 1 },
  Insurance: { books, groups: {}, counterparties, discount: { version: 1, rate_bp: 0 }, income_tax_policy: policy, income_tax_position: position, next_event_id: 1 },
  RealEstate: { books, projects: {}, presales: {}, loans: {}, receivables: { items: {}, written_off_total: "0.00" }, counterparties, budget: { operating_cash_floor: "0.00", credit_lines: {} }, capitalization_policy: { version: 1, suspension_min_days: 1 }, income_tax_policy: policy, income_tax_position: position, max_projects: 1, next_event_id: 1 },
}
