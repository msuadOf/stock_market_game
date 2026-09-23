import assert from "node:assert/strict";
import test from "node:test";
import type { PublicReportSummary } from "../types/engine.ts";
import { publicCompanyById, listedPublicCompanies } from "./company/company-catalog.ts";
import {
  formatAccountingAmount,
  formatCalendarStatus,
  formatComparison,
  formatReportPeriod,
  formatReportKind,
  formatSecondOfDay,
  reportStatementRows,
  reportViewState,
  selectVisibleReportId,
} from "./company/company-presentation.ts";
import { visibleReports } from "./company/company-view-model.ts";

function report(id: string): PublicReportSummary {
  return {
    id,
    company_id: "C-600101",
    period: "2030-03-31",
    kind: "Quarter",
    version_sequence: "1",
    supersedes: null,
    approved_date: "2030-04-01",
    approved_second_of_day: 28_800,
    published_date: "2030-04-02",
    published_second_of_day: 64_800,
    accounting: {
      total_assets: "1.00", total_liabilities: "1.00", total_equity: "1.00", closing_cash: "1.00",
      quarter_net_income: "1.00", net_income: "1.00", income_tax: "1.00", operating_cash_flow: "1.00",
      investing_cash_flow: "1.00", financing_cash_flow: "1.00", net_cash_change: "1.00",
      prior_year_net_income: { Unavailable: { reason: "NoPriorYearHistory" } },
    },
  };
}

test("财务展示保留任意精度的正负十进制金额", () => {
  assert.equal(formatAccountingAmount("9007199254740993.01"), "9007.2万亿");
  assert.equal(formatAccountingAmount("-123456789.45"), "-1.23亿");
  assert.equal(formatAccountingAmount("9999.90"), "9999.9");
});

test("报告种类、披露时刻与日历状态使用权威 DTO", () => {
  assert.equal(formatReportKind("Annual"), "年度报告");
  assert.equal(formatReportPeriod("2028-03-31"), "2028-03-31");
  assert.equal(formatSecondOfDay(64_800), "18:00");
  assert.equal(formatCalendarStatus("Trading"), "交易日");
  assert.equal(formatCalendarStatus({ Closed: "Weekend" }), "休市：周末");
  assert.equal(
    formatCalendarStatus({ Closed: { SimulatedHoliday: "SpringFestival" } }),
    "休市：模拟春节假期",
  );
});

test("缺失同比保持明确原因而不伪造成零", () => {
  assert.deepEqual(formatComparison({ Available: { amount: "-5.20" } }), {
    kind: "available",
    text: "-5.2",
  });
  assert.deepEqual(formatComparison({ Unavailable: { reason: "NoPriorYearHistory" } }), {
    kind: "unavailable",
    text: "暂无上年同期：无上年历史",
  });
});

test("四张公开报表仅从公开摘要映射，并保留所有值为字符串", () => {
  const rows = reportStatementRows({
    total_assets: "100.00",
    total_liabilities: "-20.00",
    total_equity: "120.00",
    closing_cash: "30.00",
    quarter_net_income: "4.00",
    net_income: "5.00",
    income_tax: "-1.00",
    operating_cash_flow: "6.00",
    investing_cash_flow: "-7.00",
    financing_cash_flow: "8.00",
    net_cash_change: "7.00",
    prior_year_net_income: { Unavailable: { reason: "NoPriorYearHistory" } },
  });
  assert.deepEqual(rows.map((statement) => statement.title), [
    "资产负债表",
    "利润表",
    "现金流量表",
    "所有者权益变动表",
  ]);
  assert.equal(rows[0]?.rows[0]?.amount, "100.00");
  assert.equal(rows[1]?.rows[2]?.amount, "-1.00");
  assert.equal(rows[2]?.rows[1]?.amount, "-7.00");
  assert.equal(rows[3]?.rows[1]?.amount, "5.00");
});

test("报告视图区分加载、错误、空、不可用和已就绪状态", () => {
  assert.deepEqual(reportViewState(undefined), { kind: "idle" });
  assert.deepEqual(reportViewState({ kind: "loading" }), { kind: "loading" });
  assert.deepEqual(reportViewState({ kind: "empty" }), { kind: "empty" });
  assert.deepEqual(reportViewState({ kind: "error", message: "DTO 错误" }), {
    kind: "error",
    message: "DTO 错误",
  });
  assert.deepEqual(reportViewState({ kind: "unavailable", message: "当前宿主不支持" }), {
    kind: "unavailable",
    message: "当前宿主不支持",
  });
});

test("刷新或恢复后失效的报告选择回到当前可见报告", () => {
  assert.equal(selectVisibleReportId("7", ["8", "9"]), "8");
  assert.equal(selectVisibleReportId("8", ["8", "9"]), "8");
  assert.equal(selectVisibleReportId("8", []), null);
});

test("四行业测试实体存在于展示目录但不扩展默认上市证券", () => {
  assert.deepEqual(
    ["C-TEST-IND", "C-TEST-BANK", "C-TEST-INS", "C-TEST-RE"].map((id) => publicCompanyById(id)?.industry),
    ["装备制造", "银行", "保险", "房地产开发"],
  );
  assert.deepEqual(listedPublicCompanies.map((company) => company.stockCode), [
    "600101", "002156", "300260", "600610", "000812",
  ]);
});

test("列表页面按宿主提供的报告编号顺序合并，并在失效选择后回退", () => {
  const reports = [report("7"), report("8")];
  assert.deepEqual(visibleReports({ reports: { "7": reports[0]!, "8": reports[1]! }, pages: {
    root: { kind: "ready", reportIds: ["8", "7"], nextCursor: "7" },
    "7": { kind: "ready", reportIds: [], nextCursor: null },
  } }).reports.map((item) => item.id), ["8", "7"]);
});
