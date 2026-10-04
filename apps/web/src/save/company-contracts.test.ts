import assert from "node:assert/strict"
import test from "node:test"
import { currentSaveFixture } from "./save-v2-test-fixture.ts"
import { representativeCompanyOperationsFixture } from "./company-slice-test-fixture.ts"
import { parseSetup } from "./schema/market.ts"
import { parseCompanyOperations } from "./schema/company/operations.ts"
import { parseActiveShocks, parseShockKind } from "./schema/company/policies/shock.ts"
import { parsePublicLibrary } from "./schema/company/reports.ts"
import { parseGroups } from "./schema/company/groups.ts"

test("setup 保留省略字段并严格解析显式 company config 与 groups", () => {
  const setup = currentSaveFixture().setup as Record<string, unknown>
  assert.deepEqual(parseSetup(setup, "setup"), setup)
  const empty = { ...setup, company_operations: null, groups: [] }
  assert.deepEqual(parseSetup(empty, "setup"), empty)
  const operations = representativeCompanyOperationsFixture() as any
  const config = { seed: 42, shock_params: operations.shock_params, companies: Object.values(operations.companies).map((company: any) => ({ spec: company.spec, books: company.books, flow: company.params })) }
  const explicit = { ...setup, company_operations: config, groups: [{ root: "ROOT", holdings: [{ company: "CHILD", parent_held_shares: 60 }] }] }
  assert.deepEqual(parseSetup(explicit, "setup"), explicit)
  assert.throws(() => parseSetup({ ...explicit, company_operations: {} }, "setup"), /company_operations\.seed/)
  assert.throws(() => parseSetup({ ...explicit, company_operations: { ...config, unknown: true } }, "setup"), /unknown/)
  assert.throws(() => parseSetup({ ...explicit, groups: [{ root: "ROOT", holdings: [{ company: "CHILD", parent_held_shares: "60" }] }] }, "setup"), /parent_held_shares/)
})

test("groups 拒绝重复 root、跨组重复成员、自持和不安全股份数", () => {
  const group = { root: "ROOT", holdings: [{ company: "CHILD", parent_held_shares: 60 }] }
  for (const invalid of [[group, group], [group, { root: "ROOT2", holdings: group.holdings }], [{ root: "ROOT", holdings: [{ company: "ROOT", parent_held_shares: 1 }] }]]) assert.throws(() => parseGroups(invalid), /groups/)
  for (const shares of [0, -1, Number.MAX_SAFE_INTEGER + 1]) assert.throws(() => parseGroups([{ ...group, holdings: [{ company: "CHILD", parent_held_shares: shares }] }]), /parent_held_shares/)
})

test("PaymentFailure 保留正金额与事项并拒绝无效及 active 注入", () => {
  const kind = { PaymentFailure: { what: "偿还贷款", amount: "12.34" } }
  assert.deepEqual(parseShockKind(kind, "event.kind"), kind)
  for (const amount of ["0.00", "-1.00", "1.234", "1701411834604692317316873037158841057.28", 1]) assert.throws(() => parseShockKind({ PaymentFailure: { what: "偿还贷款", amount } }, "event.kind"), /amount/)
  assert.throws(() => parseShockKind({ PaymentFailure: { what: "  ", amount: "1.00" } }, "event.kind"), /what/)
  assert.throws(() => parseActiveShocks([{ kind, amplitude_bp: 0, starts_on: "2030-01-01", expires_on: "2030-01-01" }], "active"), /active/)
})

test("CompanyOperations 严格保留 payment_failures 并拒绝缺失及非法日期/记录", () => {
  const base = representativeCompanyOperationsFixture() as any
  const company = Object.keys(base.companies)[0]
  const operations = { ...base, payment_failures: { "2030-01-01": [{ company, what: "经营支付", amount: "1.00" }] } }
  assert.deepEqual(parseCompanyOperations(operations), operations)
  const { payment_failures: _removed, ...missing } = operations
  assert.throws(() => parseCompanyOperations(missing), /payment_failures/)
  assert.throws(() => parseCompanyOperations({ ...operations, payment_failures: { "2030-02-30": [] } }), /payment_failures/)
  assert.throws(() => parseCompanyOperations({ ...operations, payment_failures: { "2030-01-01": [{ company, what: "", amount: "1.00" }] } }), /what/)
  assert.throws(() => parseCompanyOperations({ ...operations, payment_failures: { "2030-01-01": [] } }), /payment_failures/)
  assert.throws(() => parseCompanyOperations({ ...operations, payment_failures: { "2030-01-01": [{ company: "UNKNOWN", what: "经营支付", amount: "1.00" }] } }), /company/)
  assert.throws(() => parseCompanyOperations({ ...operations, payment_failures: { [operations.next_expected]: [{ company, what: "经营支付", amount: "1.00" }] } }), /payment_failures/)
})

test("公开 PaymentFailure 公告保留单日事实且拒绝伪造幅度或持续期", () => {
  const announcement = { id: 0, company: "C-1", occurred_on: "2030-01-01", published_at: { date: "2030-01-02", second_of_day: 0 }, event: { kind: { PaymentFailure: { what: "到期本金", amount: "1.00" } }, amplitude_bp: 0, starts_on: "2030-01-01", expires_on: "2030-01-01" } }
  const library = { next_seq: 1, reports: [], announcements: [announcement] }
  assert.deepEqual(parsePublicLibrary(library), library)
  for (const event of [{ ...announcement.event, amplitude_bp: 1 }, { ...announcement.event, expires_on: "2030-01-02" }]) assert.throws(() => parsePublicLibrary({ ...library, announcements: [{ ...announcement, event }] }), /event/)
})
