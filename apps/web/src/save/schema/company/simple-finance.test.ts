import assert from "node:assert/strict";
import test from "node:test";
import { parseSimpleFinanceConfig } from "./simple-finance-config.ts";
import { parseSimpleFinanceState } from "./simple-finance.ts";
import { books as baseBooks, position } from "./books/tax-owner-test-fixture.ts";
import { parseJournalEntry } from "./accounting/journal.ts";
import { simpleAccountChart } from "./simple-chart.ts";

const books = { ...baseBooks, chart: simpleAccountChart("Industrial") };

const config = { opening_lines: [{ account: "1122", side: "Debit", amount: "1000.00" }, { account: "4001", side: "Credit", amount: "1000.00" }], tax_policy: { version: 1, vat: { output_rate_bp: 1300, input_rate_bp: 1300, deductible_share_bp: 10000 }, income_tax: { rate_bp: 2500, loss_carryforward_years: 5 } }, summary_rule: "ReceivableRevenuePayableExpenses" };

test("Simple 汇总财务配置必须显式平衡且不接资金生成配置", { timeout: 10000 }, () => {
  assert.deepEqual(parseSimpleFinanceConfig(config), config);
  assert.throws(() => parseSimpleFinanceConfig({ ...config, opening_lines: [config.opening_lines[0]] }), /平衡/);
  assert.throws(() => parseSimpleFinanceConfig({ ...config, opening_lines: [] }), /非空/);
  assert.throws(() => parseSimpleFinanceConfig({ ...config, SyntheticFunding: "1000.00" }), /SyntheticFunding/);
  assert.throws(() => parseSimpleFinanceConfig({ ...config, tax_policy: { ...config.tax_policy, income_tax: { rate_bp: 10001, loss_carryforward_years: 5 } } }), /tax_policy\.income_tax/);
});

test("Simple 四种公司类别保存精确基础科目表与独立汇总科目，不冒充实际银行保险业务", { timeout: 10000 }, () => {
  for (const kind of ["Industrial", "Bank", "Insurance", "RealEstate"] as const) {
    const currentBooks = { ...baseBooks, chart: simpleAccountChart(kind) };
    const currentConfig = { ...config, opening_lines: [{ account: "simple_receivable", side: "Debit", amount: "1000.00" }, { account: "4001", side: "Credit", amount: "1000.00" }] };
    const value = { company: "C", kind, config: currentConfig, books: currentBooks, closing: { versions: [], restatements: [] }, opening_date: "2029-12-31", as_of: "2029-12-31", last_month: "2029-12", next_event_id: "1", income_tax_position: position, recognized_periods: [] };
    assert.deepEqual(parseSimpleFinanceState(value), value);
    assert.equal(currentBooks.chart.accounts.simple_revenue?.name, "Simple 汇总营业收入");
    assert.equal(currentBooks.chart.accounts.simple_revenue?.is_cash, false);
    const wrong = { ...currentBooks, chart: simpleAccountChart(kind === "Bank" ? "Industrial" : "Bank") };
    assert.throws(() => parseSimpleFinanceState({ ...value, books: wrong }), /chart\.version/);
    const missing = { ...structuredClone(currentBooks), chart: { ...currentBooks.chart, accounts: { ...currentBooks.chart.accounts } } }; delete missing.chart.accounts.simple_revenue;
    assert.throws(() => parseSimpleFinanceState({ ...value, books: missing }), /simple_revenue/);
    const drift = { ...structuredClone(currentBooks), chart: { ...currentBooks.chart, accounts: { ...currentBooks.chart.accounts } } }; drift.chart.accounts.simple_revenue = { name: "利息收入", element: "Revenue", is_cash: false, is_contra: false };
    assert.throws(() => parseSimpleFinanceState({ ...value, books: drift }), /simple_revenue/);
    assert.throws(() => parseSimpleFinanceState({ ...value, kind: "Other" }), /kind/);
    for (const account of ["missing", "simple_revenue", "simple_fixed_expense"]) assert.throws(() => parseSimpleFinanceState({ ...value, config: { ...currentConfig, opening_lines: [{ ...currentConfig.opening_lines[0], account }, currentConfig.opening_lines[1]] } }), /opening_lines\[0\].account/);
  }
});

test("期间摘要使用独立 SimplePeriodSummary 非现金标识，不伪造客户交易", { timeout: 10000 }, () => {
  const entry = { source: 1, date: "2030-01-31", kind: "SimplePeriodSummary", cash_flow: "NonCash", lines: config.opening_lines };
  assert.deepEqual(parseJournalEntry(entry, "摘要"), entry);
  for (const cashFlow of ["Operating", "Investing", "Financing"]) assert.throws(() => parseJournalEntry({ ...entry, cash_flow: cashFlow }, "摘要"), /NonCash/);
  assert.throws(() => parseJournalEntry({ ...entry, kind: "SyntheticFunding" }, "摘要"), /kind/);
  assert.throws(() => parseJournalEntry({ ...entry, kind: "SimpleMonthlySummary" }, "摘要"), /kind/);
});

test("Simple 财务状态保存同一 Books 和 Closing、规范大事件游标，拒绝未知或缺字段", { timeout: 10000 }, () => {
  const value = { company: "C-600101", kind: "Industrial", config, books, closing: { versions: [], restatements: [] }, opening_date: "2029-12-31", as_of: "2030-01-31", last_month: "2030-01", next_event_id: "18446744073709551615", income_tax_position: position, recognized_periods: [["2030-01-01", "2030-01-31"]] };
  assert.deepEqual(parseSimpleFinanceState(value), value);
  for (const key of Object.keys(value)) {
    const missing: Record<string, unknown> = { ...value }; delete missing[key];
    assert.throws(() => parseSimpleFinanceState(missing), new RegExp(key));
  }
  assert.throws(() => parseSimpleFinanceState({ ...value, cash: "1.00" }), /cash/);
  assert.throws(() => parseSimpleFinanceState({ ...value, next_event_id: 1 }), /next_event_id/);
  assert.throws(() => parseSimpleFinanceState({ ...value, last_month: "2030-02" }), /月份/);
  assert.throws(() => parseSimpleFinanceState({ ...value, opening_date: "2030-02-01" }), /开账/);
  assert.throws(() => parseSimpleFinanceState({ ...value, income_tax_position: { ...position, loss_pool: [{ origin_year: 2031, remaining: "1.00" }] } }), /loss_pool|亏损/);
  assert.throws(() => parseSimpleFinanceState({ ...value, loss_pool: [] }), /loss_pool/);
  assert.throws(() => parseSimpleFinanceState({ ...value, recognized_periods: [] }), /recognized_periods/);
  assert.throws(() => parseSimpleFinanceState({ ...value, recognized_periods: [["2030-01-02", "2030-01-31"]] }), /连续/);
});
