import assert from "node:assert/strict"
import test from "node:test"
import { parseReportSet } from "./reports.ts"

const report = {
  scope: { Standalone: "C-600101" },
  period: "2030-01",
  kind: "Monthly",
  window: ["2030-01", "2030-01"],
  version: { sequence: 1, supersedes: null, kind: "Original" },
  balance_sheet: {
    asset_lines: [["SimpleDividendSettlementAdjustment", "30.00"]],
    total_assets: "30.00",
    liability_lines: [],
    total_liabilities: "0.00",
    equity_lines: [["StatutoryReserve", "10.00"], ["RetainedEarnings", "20.00"]],
    total_equity: "30.00",
    equity_to_parent: "30.00",
    liabilities_and_equity: "30.00",
    closing_cash: "0.00",
    prior_year_end: { Available: [["SimpleDividendSettlementAdjustment", "0.00"], ["StatutoryReserve", "0.00"], ["RetainedEarnings", "0.00"]] },
  },
  income: { quarter: columns(), cumulative: columns(), prior_year: { Unavailable: { reason: "NoPriorYearHistory" } }, minority_net_income: null, net_income_to_parent: null },
  cash_flow: { operating: "0.00", investing: "0.00", financing: "0.00", net_change: "0.00", opening_cash: "0.00", closing_cash: "0.00", indirect: [] },
  equity: { opening_parent: "0.00", net_income: "0.00", other_comprehensive: "0.00", capital_contributions: "0.00", distributions: "0.00", closing_parent: "30.00", opening_minority: null, minority_net_income: null, closing_minority: null },
  notes: {
    items: [
      { code: "simple_statutory_reserve", name: "法定公积金", target: { BalanceSheet: "StatutoryReserve" }, opening: "0.00", movement: "10.00", ytd_movement: "10.00", closing: "10.00" },
      { code: "simple_dividend_settlement_asset", name: "Simple 分红账面资产调整（非现金）", target: { BalanceSheet: "SimpleDividendSettlementAdjustment" }, opening: "0.00", movement: "30.00", ytd_movement: "30.00", closing: "30.00" },
    ],
    consolidation_split_items: [],
  },
}

function columns() {
  return { operating: [], operating_subtotal: "0.00", investing: [], investing_subtotal: "0.00", financing: [], financing_subtotal: "0.00", discontinued: [], discontinued_subtotal: "0.00", income_tax: "0.00", net_income: "0.00" }
}

test("Simple 分红报表严格接受独立法定公积金与非现金账面资产行", { timeout: 10000 }, () => {
  assert.deepEqual(parseReportSet(report, "report"), report)
})

test("资产负债表行只能进入 Rust 定义的对应区，不接受未知行", { timeout: 10000 }, () => {
  for (const malformed of [
    { ...report, balance_sheet: { ...report.balance_sheet, asset_lines: [["StatutoryReserve", "10.00"]] } },
    { ...report, balance_sheet: { ...report.balance_sheet, equity_lines: [["SimpleDividendSettlementAdjustment", "10.00"]] } },
    { ...report, balance_sheet: { ...report.balance_sheet, asset_lines: [["SimpleDividendSettlementAsset", "10.00"]] } },
  ]) assert.throws(() => parseReportSet(malformed, "report"), /包含无效枚举/)
})
