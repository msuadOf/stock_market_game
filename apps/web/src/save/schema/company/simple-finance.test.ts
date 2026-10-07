import assert from "node:assert/strict";
import test from "node:test";
import { parseSimpleFinanceConfig } from "./simple-finance-config.ts";
import { parseSimpleFinanceState } from "./simple-finance.ts";
import { books as baseBooks, position } from "./books/tax-owner-test-fixture.ts";
import { parseJournalEntry } from "./accounting/journal.ts";
import { simpleAccountChart } from "./simple-chart.ts";

const books = { ...baseBooks, chart: simpleAccountChart("Industrial") };

const config = { opening_lines: [{ account: "1122", side: "Debit", amount: "1000.00" }, { account: "4001", side: "Credit", amount: "1000.00" }], tax_policy: { version: 1, vat: { output_rate_bp: 1300, input_rate_bp: 1300, deductible_share_bp: 10000 }, income_tax: { rate_bp: 2500, loss_carryforward_years: 5 } }, summary_rule: "ReceivableRevenuePayableExpenses", book_display: { investment_of_revenue_bp: 3000 }, };

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
    const value = { company: "C", kind, config: currentConfig, books: currentBooks, closing: { versions: [], restatements: [] }, opening_date: "2029-12-31", as_of: "2029-12-31", last_month: "2029-12", next_event_id: "1", income_tax_position: position, recognized_periods: [], dividends: {}, stock_distributions: {}, share_splits: {}, rights_offerings: {}, issuer_repurchases: {}, legal_facts: null, cash_book: "0.00", investment_book: "0.00" };

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
  const value = { company: "C-600101", kind: "Industrial", config, books, closing: { versions: [], restatements: [] }, opening_date: "2029-12-31", as_of: "2030-01-31", last_month: "2030-01", next_event_id: "18446744073709551615", income_tax_position: position, recognized_periods: [["2030-01-01", "2030-01-31"]], dividends: {}, stock_distributions: {}, share_splits: {}, rights_offerings: {}, issuer_repurchases: {}, legal_facts: null, cash_book: "0.00", investment_book: "0.00" };

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
  for (const key of ["dividends", "stock_distributions", "legal_facts"]) {
    const missing: Record<string, unknown> = { ...value }; delete missing[key];
    assert.throws(() => parseSimpleFinanceState(missing), new RegExp(key));
  }
  assert.throws(() => parseSimpleFinanceState({ ...value, dividends: { plan: { declaration: { plan_id: "other" } } } }), /dividends/);
  assert.throws(() => parseSimpleFinanceState({ ...value, legal_facts: { registered_capital: 10000, source_evidence: "监管披露" } }), /registered_capital/);
  assert.throws(() => parseSimpleFinanceState({ ...value, legal_facts: { registered_capital: "1000.00", source_evidence: " " } }), /source_evidence/);
});

test("Simple 分红计划 wire 要保留完整决议、公积年度与付款批次事实", { timeout: 10000 }, () => {
  const declaration = { plan_id: "dividend-plan-1", approved_on: "2030-02-01", total_gross: "100.00", registered_capital: "1000.00" };
  const plan = { declaration, declaration_source: 2, reserve: "10.00", reserve_basis_year: 2029, payments: { "payment-1": { source: 3, paid_on: "2030-02-02", amount: "30.00" } } };
  const journal = { batches: [[
    { source: 2, date: "2030-02-01", kind: "CompanyDividendDeclaration", cash_flow: "NonCash", lines: [{ account: "4103", side: "Debit", amount: "110.00" }, { account: "simple_statutory_reserve", side: "Credit", amount: "10.00" }, { account: "simple_dividend_payable", side: "Credit", amount: "100.00" }] },
    { source: 3, date: "2030-02-02", kind: "CompanyDividendPayment", cash_flow: "NonCash", lines: [{ account: "simple_dividend_payable", side: "Debit", amount: "30.00" }, { account: "simple_dividend_settlement_asset", side: "Credit", amount: "30.00" }] },
  ]], closed: [] };
  const dividendBooks = { ...books, chart: simpleAccountChart("Industrial"), journal };
  const value = { company: "C-600101", kind: "Industrial", config, books: dividendBooks, closing: { versions: [], restatements: [] }, opening_date: "2029-12-31", as_of: "2030-01-31", last_month: "2030-01", next_event_id: "4", income_tax_position: position, recognized_periods: [["2030-01-01", "2030-01-31"]], dividends: { "dividend-plan-1": plan }, stock_distributions: {}, share_splits: {}, rights_offerings: {}, issuer_repurchases: {}, legal_facts: { registered_capital: "1000.00", source_evidence: "章程及登记材料" }, cash_book: "-30.00", investment_book: "0.00" };
  assert.deepEqual(parseSimpleFinanceState(value), value);
  for (const malformed of [
    { ...plan, declaration: { ...declaration, plan_id: "wrong-id" } },
    { ...plan, reserve_basis_year: null },
    { ...plan, reserve: "0.00", reserve_basis_year: 2029 },
    { ...plan, reserve_basis_year: 2028 },
    { ...plan, payments: { "": { source: 3, paid_on: "2030-02-02", amount: "30.00" } } },
    { ...plan, payments: { "payment-1": { source: 3, paid_on: "2030-02-02", amount: "30.00", extra: true } } },
  ]) {
    const candidate = { ...value, dividends: { "dividend-plan-1": malformed } };
    assert.throws(() => parseSimpleFinanceState(candidate), /dividends/);
  }
  assert.throws(() => parseSimpleFinanceState({ ...value, dividends: { "dividend-plan-1": { ...plan, declaration_source: 3 } } }), /declaration_source|dividends/);
  assert.throws(() => parseSimpleFinanceState({ ...value, books: { ...dividendBooks, journal: { batches: [journal.batches[0].slice(0, 1)], closed: [] } } }), /dividends|simple_dividend_payable/);
  const unbalancedJournal = { batches: [...journal.batches, [{ source: 4, date: "2030-02-03", kind: "SimplePeriodSummary", cash_flow: "NonCash", lines: [{ account: "4103", side: "Debit", amount: "1.00" }] }]], closed: [] };
  assert.throws(() => parseSimpleFinanceState({ ...value, next_event_id: "5", books: { ...dividendBooks, journal: unbalancedJournal } }), /借贷不平衡/);
  const unknownAccountJournal = { batches: [...journal.batches, [{ source: 4, date: "2030-02-03", kind: "SimplePeriodSummary", cash_flow: "NonCash", lines: [{ account: "unknown", side: "Debit", amount: "1.00" }, { account: "4103", side: "Credit", amount: "1.00" }] }]], closed: [] };
  assert.throws(() => parseSimpleFinanceState({ ...value, next_event_id: "5", books: { ...dividendBooks, journal: unknownAccountJournal } }), /科目/);
  const emptyVoucherJournal = { batches: [...journal.batches, [{ source: 4, date: "2030-02-03", kind: "SimplePeriodSummary", cash_flow: "NonCash", lines: [] }]], closed: [] };
  assert.throws(() => parseSimpleFinanceState({ ...value, next_event_id: "5", books: { ...dividendBooks, journal: emptyVoucherJournal } }), /不得为空/);
  const overdrawThenRestore = { batches: [...journal.batches, [{ source: 4, date: "2030-02-03", kind: "SimplePeriodSummary", cash_flow: "NonCash", lines: [{ account: "4103", side: "Debit", amount: "5.00" }, { account: "1001", side: "Credit", amount: "5.00" }] }], [{ source: 5, date: "2030-02-04", kind: "SimplePeriodSummary", cash_flow: "NonCash", lines: [{ account: "1001", side: "Debit", amount: "5.00" }, { account: "4103", side: "Credit", amount: "5.00" }] }]], closed: [] };
  assert.throws(() => parseSimpleFinanceState({ ...value, next_event_id: "6", books: { ...dividendBooks, journal: overdrawThenRestore } }), /现金科目/);
});

test("送转入账演进注册资本后，事实按批准时点口径核对而不是当前法定事实", { timeout: 10000 }, () => {
  const base = { company: "C-600101", kind: "Industrial", config, books, closing: { versions: [], restatements: [] }, opening_date: "2029-12-31", as_of: "2030-01-31", last_month: "2030-01", next_event_id: "1", income_tax_position: position, recognized_periods: [["2030-01-01", "2030-01-31"]], dividends: {}, rights_offerings: {}, issuer_repurchases: {}, legal_facts: { registered_capital: "1000.03", source_evidence: "章程及登记材料" }, cash_book: "0.00", investment_book: "0.00" };
  const credited = { event_id: "dist-1", approval_reference: "股东大会决议", kind: "BonusShares", approved_on: "2030-02-01", new_shares: "3", par_value_per_share: "1", capital_increase: "0.03", registered_capital_at_approval: "1000.00", credited_on: "2030-02-10" };
  const value = { ...base, stock_distributions: { "dist-1": credited }, share_splits: {} };
  // 送转入账按 面值×新增股数 演进注册资本（1000.00 → 1000.03）；事实冻结批准时点注册资本。
  assert.deepEqual(parseSimpleFinanceState(value), value);
  // 批准时点口径对不上的送转声明仍被拒（1001.00 不是任何历史时点的注册资本）。
  assert.throws(() => parseSimpleFinanceState({ ...value, stock_distributions: { "dist-1": { ...credited, registered_capital_at_approval: "1001.00" } } }), /registered_capital_at_approval|注册资本/);
});

test("送转入账演进注册资本后，既有分红声明按其批准时点注册资本核对", { timeout: 10000 }, () => {
  const declaration = { plan_id: "dividend-plan-1", approved_on: "2030-02-01", total_gross: "100.00", registered_capital: "1000.00" };
  const plan = { declaration, declaration_source: 2, reserve: "10.00", reserve_basis_year: 2029, payments: { "payment-1": { source: 3, paid_on: "2030-02-02", amount: "30.00" } } };
  const journal = { batches: [[
    { source: 2, date: "2030-02-01", kind: "CompanyDividendDeclaration", cash_flow: "NonCash", lines: [{ account: "4103", side: "Debit", amount: "110.00" }, { account: "simple_statutory_reserve", side: "Credit", amount: "10.00" }, { account: "simple_dividend_payable", side: "Credit", amount: "100.00" }] },
    { source: 3, date: "2030-02-02", kind: "CompanyDividendPayment", cash_flow: "NonCash", lines: [{ account: "simple_dividend_payable", side: "Debit", amount: "30.00" }, { account: "simple_dividend_settlement_asset", side: "Credit", amount: "30.00" }] },
  ]], closed: [] };
  const dividendBooks = { ...books, chart: simpleAccountChart("Industrial"), journal };
  const credited = { event_id: "dist-1", approval_reference: "股东大会决议", kind: "BonusShares", approved_on: "2030-02-01", new_shares: "3", par_value_per_share: "1", capital_increase: "0.03", registered_capital_at_approval: "1000.00", credited_on: "2030-02-10" };
  const value = { company: "C-600101", kind: "Industrial", config, books: dividendBooks, closing: { versions: [], restatements: [] }, opening_date: "2029-12-31", as_of: "2030-01-31", last_month: "2030-01", next_event_id: "4", income_tax_position: position, recognized_periods: [["2030-01-01", "2030-01-31"]], dividends: { "dividend-plan-1": plan }, stock_distributions: { "dist-1": credited }, share_splits: {}, rights_offerings: {}, issuer_repurchases: {}, legal_facts: { registered_capital: "1000.03", source_evidence: "章程及登记材料" }, cash_book: "-30.00", investment_book: "0.00" };
  // 分红声明（2030-02-01）早于送转入账（2030-02-10）：按批准时点 1000.00 核对。
  assert.deepEqual(parseSimpleFinanceState(value), value);
  // 对不上任何历史时点注册资本的分红声明仍被拒。
  assert.throws(() => parseSimpleFinanceState({ ...value, dividends: { "dividend-plan-1": { ...plan, declaration: { ...declaration, registered_capital: "1001.00" } } } }), /注册资本/);
});
