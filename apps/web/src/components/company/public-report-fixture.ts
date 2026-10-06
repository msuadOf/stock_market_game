import type { PublicReportSummary } from "../../types/engine.ts";

export function publicReportGold(): PublicReportSummary {
  const columns = {
    operating: [{ subject: "营业收入", amount: "200.00" }, { subject: "营业成本", amount: "80.00" }],
    operating_subtotal: "120.00", investing: [], investing_subtotal: "0.00",
    financing: [{ subject: "财务费用", amount: "10.00" }], financing_subtotal: "-10.00",
    discontinued: [], discontinued_subtotal: "0.00", income_tax: "22.00", net_income: "88.00",
  };
  return {
    source: "SimulationAccounting",
    id: "7", company_id: "C-600101", period: "2030-03-31", kind: "Quarter" as const,
    version_sequence: "2", supersedes: "6", approved_date: "2030-04-01", approved_second_of_day: 28_800,
    published_date: "2030-04-02", published_second_of_day: 64_800,
    accounting: {
      total_assets: "1620.00", total_liabilities: "532.00", total_equity: "1088.00", closing_cash: "1620.00",
      quarter_net_income: "88.00", net_income: "88.00", income_tax: "22.00", operating_cash_flow: "120.00",
      report_period_net_income: "88.00",
      investing_cash_flow: "0.00", financing_cash_flow: "500.00", net_cash_change: "620.00",
      prior_year_net_income: { Unavailable: { reason: "NoPriorYearHistory" as const } },
    },
    financials: {
      scope: { Standalone: { entity_id: "C-600101" } },
      window_start: "2030-01-01", window_end: "2030-03-31",
      version_kind: { Correction: { reason: "已公布更正说明" } },
      version_supersedes: "1",
      balance_sheet: {
        asset_lines: [{ subject: "货币资金", amount: "1620.00" }], total_assets: "1620.00",
        liability_lines: [{ subject: "短期借款", amount: "500.00" }, { subject: "应付利息", amount: "10.00" }, { subject: "应交税费", amount: "22.00" }],
        total_liabilities: "532.00", equity_lines: [{ subject: "实收资本", amount: "1000.00" }, { subject: "留存收益", amount: "88.00" }],
        total_equity: "1088.00", equity_to_parent: "1088.00", liabilities_and_equity: "1620.00", closing_cash: "1620.00",
        prior_year_end: { Available: {
          lines: [{ subject: "货币资金", amount: "1000.00" }, { subject: "实收资本", amount: "1000.00" }],
          total_assets: "1000.00", total_liabilities: "0.00", total_equity: "1000.00",
          equity_to_parent: "1000.00", liabilities_and_equity: "1000.00",
        } },
      },
      income: {
        report_period: structuredClone(columns), quarter: structuredClone(columns), cumulative: structuredClone(columns),
        report_period_net_income_to_parent: null,
        prior_year: { Unavailable: { reason: "NoPriorYearHistory" as const } },
        minority_net_income: null, net_income_to_parent: null,
      },
      roe: {
        basis: "AttributableToOrdinaryShareholders",
        ordinary_roe: { Available: { numerator: "8800", denominator: "104400" } },
        adjusted_roe: { Unavailable: { reason: "MissingNonRecurringIncomeFacts" } },
        weighted_average_parent_equity_cents: { Available: { numerator: "104400", denominator: "1" } },
      },
      cash_flow: {
        operating: "120.00", investing: "0.00", financing: "500.00", net_change: "620.00",
        opening_cash: "1000.00", closing_cash: "1620.00",
        indirect: [{ subject: "净利润", amount: "88.00" }, { subject: "应付利息变动", amount: "10.00" }, { subject: "应交税费变动", amount: "22.00" }],
      },
      equity: {
        opening_parent: "1000.00", net_income: "88.00", other_comprehensive: "0.00", capital_contributions: "0.00",
        distributions: "0.00", closing_parent: "1088.00", opening_minority: null, minority_net_income: null, closing_minority: null,
      },
      notes: {
        items: [{ code: "1002", name: "银行存款", target: { BalanceSheet: "货币资金" }, opening: "1000.00", movement: "620.00", ytd_movement: "620.00", closing: "1620.00" }],
        consolidation_split_items: [],
      },
    },
  };
}

export function serdeValue(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(serdeValue);
  if (value !== null && typeof value === "object") {
    return new Map(Object.entries(value).map(([key, entry]) => [key, serdeValue(entry)]));
  }
  return value;
}
