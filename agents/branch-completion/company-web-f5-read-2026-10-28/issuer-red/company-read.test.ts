import assert from "node:assert/strict";
import test from "node:test";
import { prepareCatalogResolver } from "./catalog-resolver.ts";
import { createCompanyReadQueries, type CompanyReadQuery } from "./company-read.ts";

const identity = prepareCatalogResolver([{ id: 0, code: "600101" }], [{ id: 7, code: "实际发行人", name: "公司", listed_stock: 0 }]);
const owner = "18446744073709551615";
// 仅为 transport 合同测试；真实 Rust producer 原件由注册批另交。
const capabilities = { revenue: true, net_income: true, equity: true, cash_flow: false, full_financial_statements: false, cash_settlement: false,
  unsupported_reason: "实际投资者结算尚未接线", issued_shares: "9007199254740993", par_value_per_share: { Available: { cents: "100" } },
  registered_capital: { Unavailable: { reason: "未配置" } }, distributable_profit: { Unavailable: { reason: "未结算" } }, active_plans: [], owner_rights: [],
  action_readiness: ["CashDividend", "StockDistribution", "RightsOffering", "IssuerRepurchase", "ShareSplit"].map(kind => ({ kind, ready: false, blockers: ["尚未接线"] })),
};
const explanation = { previous: { revenue: "10.00", fixed_expense: "2.00", variable_expense: "3.00" }, cycle: "Monthly", environment_change_bp: 0, demand_contribution_bp: 0,
  revenue_segments: [], fixed_expense_segments: [], variable_expense_segments: [], revenue_noise_bp: 0, fixed_expense_noise_bp: 0, variable_expense_noise_bp: 0, restart_revenue: null, restart_source: null };
const receipt = { payment_id: "pay1", plan_id: "plan1", account: owner, paid_on: "2030-06-01", gross: "1000", rate_bp: 1000, withheld: "100" };

test("六查询按实际公司目录发送已冻结 Rust kind，本人来自响应且大整数保持字符串", { timeout: 10_000 }, async () => {
  const requests: CompanyReadQuery[] = [];
  const data: Record<string, unknown> = { CompanyCapabilities: capabilities, CompanyPeriodExplanation: explanation,
    AccountDividendTaxStatus: { mode: "AShareIndividual", identity: "Personal", stocks: [{ stock: "600101", status: "IndividualPublicMarket" }] },
    DividendTaxOutstandingViews: [{ account: owner, stock: "600101", outstanding: { numerator: "3", denominator: "2" }, latest_collection: null }],
    OwnerFlatWithholdingReceipts: [receipt], OwnerRightsOfferings: [],
  };
  const query = createCompanyReadQueries(async request => { requests.push(request); return { owner, value: { kind: request.kind, data: data[request.kind] } }; }, () => identity);
  assert.deepEqual(await query.queryCompanyCapabilities("实际发行人"), capabilities);
  assert.deepEqual(await query.queryCompanyPeriodExplanation("实际发行人", "2030-05-31"), explanation);
  assert.deepEqual(await query.queryAccountDividendTaxStatus(), data.AccountDividendTaxStatus);
  assert.deepEqual(await query.queryDividendTaxOutstandingViews(), data.DividendTaxOutstandingViews);
  assert.deepEqual(await query.queryOwnerFlatWithholdingReceipts(), [receipt]);
  assert.deepEqual(await query.queryOwnerRightsOfferings(), []);
  assert.deepEqual(requests, [{ kind: "CompanyCapabilities", data: { company: 7 } }, { kind: "CompanyPeriodExplanation", data: { company: 7, period_end: "2030-05-31" } },
    { kind: "AccountDividendTaxStatus" }, { kind: "DividendTaxOutstandingViews" }, { kind: "OwnerFlatWithholdingReceipts" }, { kind: "OwnerRightsOfferings" }]);
});

test("未知公司与非法日期在发送前拒绝，不能将公司代码当 StockId 或数字转换", { timeout: 10_000 }, async () => {
  let sent = 0;
  const query = createCompanyReadQueries(async () => { sent++; throw new Error("不应发送"); }, () => identity);
  await assert.rejects(query.queryCompanyCapabilities("600101"), /未知公司/);
  await assert.rejects(query.queryCompanyPeriodExplanation("实际发行人", "2030-02-30"), /日期|CivilDate/);
  assert.equal(sent, 0);
});

test("查询响应严格拒绝缺失或数字 owner、错 kind、未知字段与其他账户款项", { timeout: 10_000 }, async () => {
  for (const raw of [
    { value: { kind: "OwnerFlatWithholdingReceipts", data: [receipt] } },
    { owner: 0, value: { kind: "OwnerFlatWithholdingReceipts", data: [receipt] } },
    { owner, value: { kind: "OwnerRightsOfferings", data: [] } },
    { owner, value: { kind: "OwnerFlatWithholdingReceipts", data: [receipt], extra: true } },
    { owner, value: { kind: "OwnerFlatWithholdingReceipts", data: [{ ...receipt, account: "0" }] } },
  ]) {
    const query = createCompanyReadQueries(async () => raw, () => identity);
    await assert.rejects(query.queryOwnerFlatWithholdingReceipts(), /owner|kind|extra|account/);
  }
});

test("领域与 transport 失败原样拒绝，不冒充空权益或零未缴税额", { timeout: 10_000 }, async () => {
  const failure = new Error("当前会话未配置公司行为状态");
  const query = createCompanyReadQueries(async () => { throw failure; }, () => identity);
  await assert.rejects(query.queryOwnerRightsOfferings(), error => error === failure);
  await assert.rejects(query.queryDividendTaxOutstandingViews(), error => error === failure);
});

test("权益 issuer 与股票必须属于同代目录中的同一发行人，能力摘要也不能串公司", { timeout: 10_000 }, async () => {
  const catalog = prepareCatalogResolver([{ id: 0, code: "600101" }, { id: 1, code: "600102" }], [
    { id: 7, code: "实际发行人", name: "公司一", listed_stock: 0 }, { id: 8, code: "其他发行人", name: "公司二", listed_stock: 1 },
  ]);
  const rights = { event_id: "r1", stock: "600101", issuer: 8, stage: "Entitled", payment_window: "Open", price_per_share: "100",
    payment_start_on: "2030-06-01", payment_deadline_on: "2030-06-03", ex_rights_on: "2030-06-04", settlement_on: "2030-06-05",
    owner_entitlement: { rights_shares: "10", lock_until: null }, open_subscription_remaining_shares: null, queued_subscription: null, settled_subscription: null };
  const query = createCompanyReadQueries(async request => ({ owner, value: { kind: request.kind, data: request.kind === "OwnerRightsOfferings" ? [rights] : {
    ...capabilities, owner_rights: [{ event_id: "r1", stock: "600102", stage: "Entitled", payment_window: "Open", entitled_shares: "10", open_subscription_remaining: null }],
  } } }), () => catalog);
  await assert.rejects(query.queryOwnerRightsOfferings(), /发行|issuer/);
  await assert.rejects(query.queryCompanyCapabilities("实际发行人"), /发行|issuer/);
});
