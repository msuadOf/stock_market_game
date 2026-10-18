import assert from "node:assert/strict";
import test from "node:test";
import { normalizePublicReportById, normalizePublicReportPage } from "../../host/serde-normalize.ts";
import { reportStatementRows } from "./company-presentation.ts";
import { publicReportGold, serdeValue } from "./public-report-fixture.ts";

test("完整已公布金样通过 JSON 与 serde Map 边界，精确金额不丢失", () => {
  const report = publicReportGold();
  report.financials.balance_sheet.asset_lines[0]!.amount = "9007199254740993.01";
  assert.doesNotThrow(() => normalizePublicReportById(serdeValue(report)));
  assert.deepEqual(normalizePublicReportById(serdeValue(report)), report);
  assert.deepEqual(normalizePublicReportPage(serdeValue({ reports: [report], next_cursor: "7" })), { reports: [report], next_cursor: "7" });
});

test("四表取已公布科目、现金调节和真实权益滚动，不从汇总拼造", () => {
  const report = publicReportGold();
  const statements = reportStatementRows(report.financials);
  assert.equal(statements[0]?.rows.find((row) => row.subject === "货币资金")?.amount, "1620.00");
  assert.equal(statements[1]?.rows.find((row) => row.subject === "营业收入")?.amount, "200.00");
  assert.equal(statements[2]?.rows.find((row) => row.subject === "应付利息变动")?.amount, "10.00");
  assert.equal(statements[3]?.rows.find((row) => row.subject === "期初所有者权益")?.amount, "1000.00");
  assert.equal(statements[3]?.rows.find((row) => row.subject === "对所有者分配")?.amount, "0.00");
});

test("公共边界拒绝缺五产物、数值金额、非法比较项和私有字段", () => {
  const missing = publicReportGold();
  delete (missing.financials as Partial<typeof missing.financials>).notes;
  assert.throws(() => normalizePublicReportById(missing), /financials.*字段/);
  const numeric = publicReportGold();
  (numeric.financials.notes.items[0] as unknown as Record<string, unknown>).opening = 1000;
  assert.throws(() => normalizePublicReportById(numeric), /notes.*opening.*精确十进制字符串/);
  const privateState = publicReportGold();
  Object.assign(privateState.financials, { books: { journal: [] } });
  assert.throws(() => normalizePublicReportById(privateState), /financials.*字段/);
  const comparative = publicReportGold();
  Object.assign(comparative.financials.income.prior_year, { Available: comparative.financials.income.quarter });
  assert.throws(() => normalizePublicReportById(comparative), /prior_year.*字段/);
});

test("公开报告拒绝摘要与四表跨层漂移，以及单体少数股东列", () => {
  const drift = publicReportGold();
  drift.accounting.net_income = "89.00";
  assert.throws(() => normalizePublicReportById(drift), /accounting.net_income.*不一致/);
  const minority = publicReportGold();
  Object.assign(minority.financials.equity, { closing_minority: "1.00" });
  assert.throws(() => normalizePublicReportById(minority), /scope.*少数股东/);
});

test("公开报告拒绝不存在日期、未结束窗口与更正关系漂移", () => {
  const invalidDate = publicReportGold();
  invalidDate.approved_date = "2030-02-30";
  assert.throws(() => normalizePublicReportById(invalidDate), /approved_date.*日期/);
  const future = publicReportGold();
  future.financials.window_end = "2030-04-03";
  assert.throws(() => normalizePublicReportById(future), /window.*时序|window.*不一致/);
  const correction = publicReportGold();
  correction.supersedes = null;
  assert.throws(() => normalizePublicReportById(correction), /version_kind.*更正/);
});

test("单体报表不冒充归母权益，不为行业补造未披露科目", () => {
  for (const [asset, liability] of [["固定资产", "短期借款"], ["贷款及垫款", "吸收存款"], ["应收保费", "保险合同负债"], ["开发存货", "合同负债"]]) {
    const financials = publicReportGold().financials;
    financials.balance_sheet.asset_lines = [{ subject: asset!, amount: "1.00" }];
    financials.balance_sheet.liability_lines = [{ subject: liability!, amount: "2.00" }];
    const statement = reportStatementRows(financials)[0]!;
    assert.equal(statement.rows.find((row) => row.subject === asset)?.amount, "1.00");
    assert.equal(statement.rows.find((row) => row.subject === liability)?.amount, "2.00");
    assert.ok(statement.rows.every((row) => !row.subject.includes("母公司")));
    assert.ok(!statement.rows.some((row) => row.subject === "少数股东权益"));
  }
});

test("可用上年年末合计必须呈现精确值，不冒充未列报", () => {
  const statement = reportStatementRows(publicReportGold().financials)[0]!;
  assert.equal(statement.rows.find((row) => row.subject === "资产总计")?.comparisons?.[0], "1000.00");
  assert.equal(statement.rows.find((row) => row.subject === "负债合计")?.comparisons?.[0], "0.00");
  assert.equal(statement.rows.find((row) => row.subject === "所有者权益合计")?.comparisons?.[0], "1000.00");
  assert.equal(statement.rows.find((row) => row.subject === "负债和所有者权益总计")?.comparisons?.[0], "1000.00");
});

test("现金流表保留期末现金，资产负债表不把期末现金重复当作科目", () => {
  const statements = reportStatementRows(publicReportGold().financials);
  assert.ok(!statements[0]!.rows.some((row) => row.subject === "期末现金"));
  assert.equal(statements[2]!.rows.find((row) => row.subject === "期末现金")?.amount, "1620.00");
  assert.equal(statements[0]!.rows.find((row) => row.subject === "货币资金")?.amount, "1620.00");
});
