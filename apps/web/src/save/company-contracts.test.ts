import assert from "node:assert/strict"
import test from "node:test"
import { representativeCurrentSaveFixture } from "./representative-save-test-fixture.ts"
import { representativeCompanyOperationsFixture } from "./company-slice-test-fixture.ts"
import { parseSetup } from "./schema/market.ts"
import { parseCompanyOperations, parseCompanyOperationsConfig } from "./schema/company/operations.ts"
import { parseActiveShocks, parseShockKind } from "./schema/company/policies/shock.ts"
import { parsePublicLibrary } from "./schema/company/reports.ts"
import { parseGroups } from "./schema/company/groups.ts"
import { parseSaveSlot } from "./save-schema.ts"

test("当前存档保留 company_system 与职责运行时字段且拒绝旧仿真和代际字段", { timeout: 10_000 }, () => {
  const save = representativeCurrentSaveFixture()
  assert.deepEqual(parseSaveSlot(save), save)
  const { company_system: _removed, ...missingSystem } = save
  assert.throws(() => parseSaveSlot(missingSystem), /company_system/)
  assert.throws(() => parseSaveSlot({ ...save, groups: [{ root: "ROOT", holdings: [{ company: "CHILD", parent_held_shares: 60 }] }] }), /groups/)
  for (const schema_version of [1, 2, 3]) assert.throws(() => parseSaveSlot({ ...save, schema_version }), /schema_version/)
  assert.throws(() => parseSaveSlot({ ...save, runtime_v2: save.runtime_state }), /runtime_v2/)
})

test("setup 严格解析现行 company_system，旧 CompanyOperations 配置在独立低层校验", () => {
  const setup = representativeCurrentSaveFixture().setup as Record<string, unknown>
  assert.deepEqual(parseSetup(setup, "setup"), setup)
  const empty = { ...setup, company_operations: null, groups: [] }
  assert.throws(() => parseSetup(empty, "setup"), /company_operations|groups/)
  const operations = representativeCompanyOperationsFixture() as any
  const config = { seed: 42, shock_params: operations.shock_params, companies: Object.values(operations.companies).map((company: any) => ({ spec: company.spec, books: company.books, flow: company.params })) }
  assert.deepEqual(parseCompanyOperationsConfig(config, "company_operations"), config)
  assert.throws(() => parseCompanyOperationsConfig({}, "company_operations"), /company_operations\.seed/)
  assert.throws(() => parseCompanyOperationsConfig({ ...config, unknown: true }, "company_operations"), /unknown/)
  assert.throws(() => parseGroups([{ root: "ROOT", holdings: [{ company: "CHILD", parent_held_shares: "60" }] }]), /parent_held_shares/)
})

test("groups 拒绝重复 root、跨组重复成员、自持和不安全股份数", () => {
  const group = { root: "ROOT", holdings: [{ company: "CHILD", parent_held_shares: 60 }] }
  assert.deepEqual(parseGroups([group]), [group])
  for (const invalid of [[group, group], [group, { root: "ROOT2", holdings: group.holdings }], [{ root: "ROOT", holdings: [{ company: "ROOT", parent_held_shares: 1 }] }]]) assert.throws(() => parseGroups(invalid), /groups/)
  for (const shares of [0, -1, Number.MAX_SAFE_INTEGER + 1]) assert.throws(() => parseGroups([{ ...group, holdings: [{ company: "CHILD", parent_held_shares: shares }] }]), /parent_held_shares/)
})

test("PaymentFailure 保留正金额与事项并拒绝无效及 active 注入", () => {
  const kind = { PaymentFailure: { what: "偿还贷款", amount: "12.34", obligation_status: "ContractualOverdue" } }
  assert.deepEqual(parseShockKind(kind, "event.kind"), kind)
  for (const amount of ["0.00", "-1.00", "1.234", "1701411834604692317316873037158841057.28", 1]) assert.throws(() => parseShockKind({ PaymentFailure: { what: "偿还贷款", amount, obligation_status: "ContractualOverdue" } }, "event.kind"), /amount/)
  assert.throws(() => parseShockKind({ PaymentFailure: { what: "  ", amount: "1.00", obligation_status: "ContractualOverdue" } }, "event.kind"), /what/)
  assert.throws(() => parseActiveShocks([{ kind, amplitude_bp: 0, starts_on: "2030-01-01", expires_on: "2030-01-01" }], "active"), /active/)
})

test("CompanyOperations 严格保留 payment_failures 并拒绝缺失及非法日期/记录", () => {
  const base = representativeCompanyOperationsFixture() as any
  const company = Object.keys(base.companies)[0]
  const operations = { ...base, payment_failures: { "2030-01-01": [{ company, what: "经营支付", amount: "1.00", obligation_status: "UncommittedExpense" }] } }
  assert.deepEqual(parseCompanyOperations(operations), operations)
  const { payment_failures: _removed, ...missing } = operations
  assert.throws(() => parseCompanyOperations(missing), /payment_failures/)
  assert.throws(() => parseCompanyOperations({ ...operations, payment_failures: { "2030-02-30": [] } }), /payment_failures/)
  assert.throws(() => parseCompanyOperations({ ...operations, payment_failures: { "2030-01-01": [{ company, what: "", amount: "1.00", obligation_status: "UncommittedExpense" }] } }), /what/)
  assert.throws(() => parseCompanyOperations({ ...operations, payment_failures: { "2030-01-01": [] } }), /payment_failures/)
  assert.throws(() => parseCompanyOperations({ ...operations, payment_failures: { "2030-01-01": [{ company: "UNKNOWN", what: "经营支付", amount: "1.00", obligation_status: "UncommittedExpense" }] } }), /company/)
  assert.throws(() => parseCompanyOperations({ ...operations, payment_failures: { [operations.next_expected]: [{ company, what: "经营支付", amount: "1.00" }] } }), /payment_failures/)
})

test("公开 Shock 公告使用内容联合类型并保留 PaymentFailure 单日事实", { timeout: 10_000 }, () => {
  const announcement = { id: 0, company: "C-1", occurred_on: "2030-01-01", published_at: { date: "2030-01-02", second_of_day: 0 }, content: { kind: "Shock", value: { kind: { PaymentFailure: { what: "到期本金", amount: "1.00", obligation_status: "ContractualOverdue" } }, amplitude_bp: 0, starts_on: "2030-01-01", expires_on: "2030-01-01" } } }
  const library = { next_seq: 1, reports: [], announcements: [announcement] }
  assert.deepEqual(parsePublicLibrary(library).announcements[0]?.content, { Shock: announcement.content.value })
  for (const value of [{ ...announcement.content.value, amplitude_bp: 1 }, { ...announcement.content.value, expires_on: "2030-01-02" }]) assert.throws(() => parsePublicLibrary({ ...library, announcements: [{ ...announcement, content: { kind: "Shock", value } }] }), /content/)
  assert.throws(() => parsePublicLibrary({ ...library, announcements: [{ ...announcement, event: { ...announcement.content.value } }] }), /event/)
})

test("已批准现金分红公告严格保留方案与核准总额并校验身份、日期和金额", { timeout: 10_000 }, () => {
  const plan = { plan_id: "DIV-1", issuer: "C-1", stock: "600101", exchange: "Shanghai", formula: "StandardCashOnly", approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-02", ex_dividend_on: "2030-01-03", payable_on: "2030-01-03", gross_per_share: "10", distributable_amount: "1000" }
  const announcement = { id: 0, company: "C-1", occurred_on: "2030-01-02", published_at: { date: "2030-01-03", second_of_day: 0 }, content: { kind: "CashDividend", value: { plan, total_gross: "100" } } }
  const library = { next_seq: 1, reports: [], announcements: [announcement] }
  assert.deepEqual(parsePublicLibrary(library).announcements[0]?.content, { CashDividend: announcement.content.value })
  for (const invalid of [
    { ...announcement.content.value, plan: { ...plan, issuer: "C-2" } },
    { ...announcement.content.value, plan: { ...plan, announced_on: "2030-01-01" } },
    { ...announcement.content.value, total_gross: "0" },
    { ...announcement.content.value, total_gross: "1001" },
    { ...announcement.content.value, plan: { ...plan, gross_per_share: "0" } },
  ]) assert.throws(() => parsePublicLibrary({ ...library, announcements: [{ ...announcement, content: { kind: "CashDividend", value: invalid } }] }), /content/)
})
