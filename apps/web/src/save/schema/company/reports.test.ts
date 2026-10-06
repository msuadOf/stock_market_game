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
  income: { report_period: columns(), report_period_net_income_to_parent: null, quarter: columns(), cumulative: columns(), prior_year: { Unavailable: { reason: "NoPriorYearHistory" } }, minority_net_income: null, net_income_to_parent: null },
  roe: { basis: "AttributableToOrdinaryShareholders", ordinary_roe: { Available: { numerator: "0", denominator: "1" } }, adjusted_roe: unavailable("MissingNonRecurringIncomeFacts"), weighted_average_parent_equity_cents: { Available: { numerator: "1", denominator: "1" } } },
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

function unavailable(reason: string) { return { Unavailable: { reason } } }

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

test("ReportSet 严格解析必需的窗口利润与 ROE 精确值，不默认缺字段", { timeout: 10000 }, () => {
  const valid = { ...report, income: { ...report.income, report_period: { ...columns(), operating_subtotal: "12.00", net_income: "12.00" } }, equity: { ...report.equity, net_income: "12.00" }, roe: { basis: "AttributableToOrdinaryShareholders", ordinary_roe: { Available: { numerator: "1200", denominator: "5000" } }, adjusted_roe: unavailable("MissingNonRecurringIncomeFacts"), weighted_average_parent_equity_cents: { Available: { numerator: "5000", denominator: "1" } } } }
  assert.deepEqual(parseReportSet(valid, "report"), valid)
  const { roe: _roe, ...missingRoe } = valid
  const { report_period: _reportPeriod, ...missingPeriodIncome } = valid.income
  const { report_period_net_income_to_parent: _reportPeriodParentProfit, ...missingPeriodParentProfit } = valid.income
  assert.throws(() => parseReportSet(missingRoe, "report"), /roe/)
  assert.throws(() => parseReportSet({ ...valid, income: missingPeriodIncome }, "report"), /report_period/)
  assert.throws(() => parseReportSet({ ...valid, income: missingPeriodParentProfit }, "report"), /report_period_net_income_to_parent/)
  for (const value of [
    { ...valid, roe: { ...valid.roe, ordinary_roe: { Available: { numerator: "-0", denominator: "2" } } } },
    { ...valid, roe: { ...valid.roe, ordinary_roe: { Available: { numerator: "1", denominator: "0" } } } },
    { ...valid, roe: { ...valid.roe, basis: { Unsupported: { reason: "UnknownReason" } } } },
    ...["NonPositiveAverageEquity", "MissingNonRecurringIncomeFacts"].map((reason) => ({ ...valid, roe: { basis: { Unsupported: { reason } }, ordinary_roe: unavailable(reason), adjusted_roe: unavailable(reason), weighted_average_parent_equity_cents: unavailable(reason) } })),
    { ...valid, roe: { ...valid.roe, adjusted_roe: unavailable("UnknownReason") } },
  ]) assert.throws(() => parseReportSet(value, "report"), /roe/)
  assert.throws(() => parseReportSet({ ...valid, roe: { ...valid.roe, ordinary_roe: { Available: { numerator: "1201", denominator: "5000" } } } }, "report"), /roe.*一致/)
  assert.throws(() => parseReportSet({ ...valid, window: ["2029-12", "2030-01"] }, "report"), /window/)
  assert.throws(() => parseReportSet({ ...valid, income: { ...valid.income, report_period: { ...valid.income.report_period, net_income: "11.00" } } }, "report"), /report_period.net_income/)
})

test("ReportSet 归母窗口净利润必须严格按范围校验并勾稽 ROE 与权益表", { timeout: 10000 }, () => {
  const consolidated = {
    ...report,
    period: "2030-02",
    window: ["2030-02", "2030-02"],
    scope: { Consolidated: "C-600101" },
    income: { ...report.income, report_period: { ...columns(), operating_subtotal: "12.00", net_income: "12.00" }, quarter: { ...columns(), operating_subtotal: "999.00", net_income: "999.00" }, cumulative: { ...columns(), operating_subtotal: "999.00", net_income: "999.00" }, report_period_net_income_to_parent: "12.00", net_income_to_parent: "999.00" },
    equity: { ...report.equity, net_income: "12.00" },
    roe: { basis: { Unsupported: { reason: "ConsolidatedAttributionFactsUnavailable" } }, ordinary_roe: unavailable("ConsolidatedAttributionFactsUnavailable"), adjusted_roe: unavailable("ConsolidatedAttributionFactsUnavailable"), weighted_average_parent_equity_cents: unavailable("ConsolidatedAttributionFactsUnavailable") },
  }
  assert.deepEqual(parseReportSet(consolidated, "report"), consolidated)
  assert.throws(() => parseReportSet({ ...report, income: { ...report.income, report_period_net_income_to_parent: "0.00" } }, "report"), /报告范围一致/)
  assert.throws(() => parseReportSet({ ...consolidated, income: { ...consolidated.income, report_period_net_income_to_parent: null } }, "report"), /报告范围一致/)
  const forgedAvailableRoe = { basis: "AttributableToOrdinaryShareholders", ordinary_roe: { Available: { numerator: "1200", denominator: "1" } }, adjusted_roe: unavailable("MissingNonRecurringIncomeFacts"), weighted_average_parent_equity_cents: { Available: { numerator: "1", denominator: "1" } } }
  assert.throws(() => parseReportSet({ ...consolidated, roe: forgedAvailableRoe }, "report"), /合并报告.*不支持 ROE/)
  assert.throws(() => parseReportSet({ ...consolidated, equity: { ...consolidated.equity, net_income: "11.00" } }, "report"), /equity.net_income.*同窗口归属/)
})
