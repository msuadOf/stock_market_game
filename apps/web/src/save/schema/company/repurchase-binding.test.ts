import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { parseCompanySystemState, parseSavedSessionCompanies, parseSessionCorporateActions } from "./state.ts";
import { parseRawSaveJson } from "../raw-json.ts";
import { materializeSaveValue } from "../primitives.ts";

const historical = JSON.parse(readFileSync(new URL("../../fixtures/source-b/generated-company-state-registry.json", import.meta.url), "utf8")) as { company_state: { CompanySystem: { corporate_actions: Record<string, unknown> } } };
const oldActions = historical.company_state.CompanySystem.corporate_actions;
// synthetic：按冻结 Rust struct 构造输入边界，不是批准、注资、执行或恢复见证。
const binding = {
  event_id: "repurchase-boundary", issuer: 0, stock: "600101", account: "9007199254740993",
  plan_id: { owner: "9007199254740993", sequence: "18446744073709551615" },
  funding_committed: true, funding_amount: "9007199254740993",
  applied_fact_cursor: "18446744073709551615", closing_state: "Active",
};
// synthetic：新字段只投影到测试输入，保留历史 JSON 原件与缺字段拒绝断言。
const actions = (bindings: unknown = [binding]) => ({ ...oldActions,
  registries: (oldActions.registries as Record<string, unknown>[]).map(registry => ({ ...registry, pending_opening: null })),
  repurchase_execution_bindings: bindings });

test("回购 binding 新容器字段必填，旧完整 actions 不静默补空数组（synthetic）", () => {
  assert.throws(() => parseSessionCorporateActions(oldActions, "actions"), /repurchase_execution_bindings/);
  assert.deepEqual(parseSessionCorporateActions(actions([]), "actions"), actions([]));
  for (const value of [null, {}, "[]", [null], Array(1)]) assert.throws(() => parseSessionCorporateActions(actions(value), "actions"), /repurchase_execution_bindings/);
});

test("回购 binding 严格保留 PlanId、Money 分与 canonical u64 游标（synthetic）", () => {
  for (const closing_state of ["Active", "Draining", "Closed"]) {
    const value = actions([{ ...binding, closing_state }]);
    assert.deepEqual(parseSessionCorporateActions(value, "actions"), value);
    assert.deepEqual(materializeSaveValue(parseSessionCorporateActions(parseRawSaveJson(JSON.stringify(value)), "actions")), value);
  }
  for (const funding_committed of [false, true]) {
    const value = actions([{ ...binding, funding_committed, funding_amount: "0", applied_fact_cursor: "0" }]);
    assert.deepEqual(parseSessionCorporateActions(value, "actions"), value);
  }
});

test("回购 binding 九字段及 PlanId 两字段全部必填且拒绝额外键（synthetic）", () => {
  for (const key of Object.keys(binding)) {
    const value: Record<string, unknown> = { ...binding }; delete value[key];
    assert.throws(() => parseSessionCorporateActions(actions([value]), "actions"), new RegExp(key));
  }
  for (const key of ["owner", "sequence"]) {
    const plan: Record<string, unknown> = { ...binding.plan_id }; delete plan[key];
    assert.throws(() => parseSessionCorporateActions(actions([{ ...binding, plan_id: plan }]), "actions"), new RegExp(`plan_id\\.${key}`));
  }
  for (const value of [{ ...binding, extra: true }, { ...binding, plan_id: { ...binding.plan_id, extra: true } }]) assert.throws(() => parseSessionCorporateActions(actions([value]), "actions"), /extra/);
});

test("回购 binding 身份、分金额、枚举与布尔值拒绝错误 wire（synthetic）", () => {
  const cases: [string, unknown][] = [
    ...["0", -1, 4294967296].map(value => ["issuer", value] as [string, unknown]),
    ["stock", 0], ["event_id", null], ["closing_state", "Unknown"], ["closing_state", { Active: {} }],
    ["funding_committed", 0], ["funding_committed", "true"],
    ...[0, "01", "+1", "-0", "18446744073709551616"].flatMap(value => [["account", value], ["applied_fact_cursor", value]] as [string, unknown][]),
    ...[0, "01", "+1", "-0", "9223372036854775808"].map(value => ["funding_amount", value] as [string, unknown]),
  ];
  for (const [key, value] of cases) assert.throws(() => parseSessionCorporateActions(actions([{ ...binding, [key]: value }]), "actions"), new RegExp(key));
  for (const key of ["owner", "sequence"]) for (const value of [0, "01", "18446744073709551616"]) assert.throws(() => parseSessionCorporateActions(actions([{ ...binding, plan_id: { ...binding.plan_id, [key]: value } }]), "actions"), new RegExp(`plan_id\\.${key}`));
});

test("回购 binding 原始 JSON 重复 event、cursor 或 PlanId 身份键拒绝（synthetic）", () => {
  const raw = JSON.stringify(actions());
  for (const field of ["event_id", "applied_fact_cursor", "owner", "sequence"]) {
    const bad = raw.replace(`"${field}":`, `"${field}":null,"${field}":`);
    assert.notEqual(raw, bad);
    assert.throws(() => parseSessionCorporateActions(parseRawSaveJson(bad), "actions"), /重复 JSON 键/);
  }
});

// fixture 来自 issuer_repurchase_approval_tests 的 approval_c_session_01 和 _06 的公司状态输出。
for (const [name, funded] of [["funded-companies.json", true], ["disabled-companies.json", false]] as const) test(`真实 Rust ${name} 公司状态对象/raw 无损`, { timeout: 10_000 }, () => {
  const bytes = readFileSync(new URL(`../../fixtures/repurchase-binding/${name}`, import.meta.url));
  const value = JSON.parse(bytes.toString("utf8")) as { CompanySystem: { system: unknown; corporate_actions: unknown } };
  const system = parseCompanySystemState(value.CompanySystem.system, "companies.CompanySystem.system");
  const setup = { company_system: { mode: "Simple" as const, config: system.implementation.state.config } };
  const parsed = parseSavedSessionCompanies(value, setup, "companies");
  assert.deepEqual(parsed, value);
  assert.deepEqual(materializeSaveValue(parseSavedSessionCompanies(parseRawSaveJson(bytes.toString("utf8")), setup, "companies")), value);
  assert.ok("CompanySystem" in parsed);
  const actions = parsed.CompanySystem.corporate_actions;
  assert.equal(actions.repurchase_execution_bindings.length, funded ? 1 : 0);
  assert.equal(actions.issuer_repurchases.length, funded ? 1 : 0);
  if (funded) {
    const binding = actions.repurchase_execution_bindings[0];
    const book = actions.issuer_repurchases[0];
    assert.deepEqual(binding, { event_id: "批准一", issuer: 0, stock: "000001", account: "3", plan_id: { owner: "3", sequence: "0" }, funding_committed: true, funding_amount: "100000", applied_fact_cursor: "0", closing_state: "Active" });
    assert.equal(binding.event_id, book.plan.event_id);
    assert.equal(binding.issuer, book.plan.issuer);
    assert.equal(binding.stock, book.plan.stock);
    assert.equal(binding.funding_amount, book.plan.total_budget);
    assert.equal(book.status, "Approved");
  }
});
