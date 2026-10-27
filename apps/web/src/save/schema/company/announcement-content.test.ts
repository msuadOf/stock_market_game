import assert from "node:assert/strict"
import test from "node:test"
import { parseAnnouncementContent } from "./announcement-content.ts"
import { SaveSchemaError } from "../primitives.ts"

const path = "public_library.announcements[0].content"
const common = {
  event_id: "corporate-1", approval_reference: "approval-1", issuer: 4_294_967_295,
  stock: "600000", exchange: "Sse", approved_on: "2030-01-01", announced_on: "2030-01-02",
  registered_on: "2030-01-03", ex_rights_on: "2030-01-04",
}
const cashPlan = {
  plan_id: "dividend-1", issuer: 0, stock: "000001", exchange: "Szse", formula: "StandardCashOnly",
  approved_on: "2030-01-01", announced_on: "2030-01-02", registered_on: "2030-01-03",
  ex_dividend_on: "2030-01-04", payable_on: "2030-01-04", gross_per_share: "1", distributable_amount: "9007199254740993",
}
const rightsPlan = {
  ...common, payment_start_on: "2030-01-04", payment_deadline_on: "2030-01-07",
  ex_rights_on: "2030-01-08", settlement_on: "2030-01-09", listing_on: "2030-01-09",
  price_per_share: "9007199254740993", npc_subscription_strategy: "FullByDefault",
  mode: { RightsToAllShareholders: { shares_per_existing_share_micros: "100000" } },
}
const repurchasePlan = {
  event_id: "repurchase-1", approval_reference: "approval-1", issuer: 0, stock: "600000", exchange: "Sse",
  approved_on: "2030-01-01", announced_on: "2030-01-02", window_start_on: "2030-01-03", window_deadline_on: "2030-01-04",
  price_cap_per_share: "1000", total_budget: "9007199254740993", max_shares: "18446744073709551615",
  purpose: "ReduceCapital", completion_policy: "CancelOnCompletion", approval_unit_event_id: null,
}
const variants = [
  { kind: "Shock", value: { kind: "MarketDemandShift", amplitude_bp: -100, starts_on: "2030-01-01", expires_on: "2030-01-02" } },
  { kind: "CashDividend", value: { plan: cashPlan, total_gross: "9007199254740993" } },
  { kind: "RightsOffering", value: { plan: rightsPlan } },
  { kind: "IssuerRepurchase", value: { plan: repurchasePlan } },
  { kind: "ShareSplit", value: { ...common, direction: "Consolidate", ratio: "2" } },
  { kind: "StockDistribution", value: { ...common, kind: "CapitalReserveConversion", shares_per_existing_share_micros: "100000", approved_total_new_shares: "18446744073709551615" } },
]

function rejects(value: unknown, expectedPath: string): void {
  assert.throws(() => parseAnnouncementContent(value, path), (error) => error instanceof SaveSchemaError && error.path === expectedPath)
}

for (const fixture of variants) {
  test(`${fixture.kind} 保留当前 Rust 公告 wire 及精确数值`, { timeout: 10000 }, () => {
    assert.deepEqual(parseAnnouncementContent(fixture, path), fixture)
  })
  test(`${fixture.kind} 拒绝缺失与未知字段并报告完整路径`, { timeout: 10000 }, () => {
    rejects({ ...fixture, unknown: true }, `${path}.unknown`)
    rejects({ kind: fixture.kind }, `${path}.value`)
    rejects({ ...fixture, value: { ...fixture.value, unknown: true } }, `${path}.value.unknown`)
    for (const field of Object.keys(fixture.value)) {
      const body: Record<string, unknown> = { ...fixture.value }
      delete body[field]
      rejects({ ...fixture, value: body }, `${path}.value.${field}`)
    }
    if ("plan" in fixture.value && fixture.value.plan !== undefined) {
      for (const field of Object.keys(fixture.value.plan)) {
        const plan: Record<string, unknown> = { ...fixture.value.plan }
        delete plan[field]
        rejects({ ...fixture, value: { ...fixture.value, plan } }, `${path}.value.plan.${field}`)
      }
      rejects({ ...fixture, value: { ...fixture.value, plan: { ...fixture.value.plan, unknown: true } } }, `${path}.value.plan.unknown`)
    }
  })
}

test("拒绝未知公告种类和旧 event 包装", { timeout: 10000 }, () => {
  rejects({ kind: "Unknown", value: {} }, `${path}.kind`)
  rejects({ Shock: variants[0]!.value }, `${path}.kind`)
  rejects({ kind: "ShareSplit", value: { plan: { ...common, direction: "Split", ratio: "2" } } }, `${path}.value.event_id`)
})

test("股份与比例只接受 canonical u64 字符串", { timeout: 10000 }, () => {
  for (const invalid of [2, "02", "-1", "+2", "2.0", "2e1", " 2", "2\n", "18446744073709551616"]) {
    rejects({ kind: "ShareSplit", value: { ...common, direction: "Split", ratio: invalid } }, `${path}.value.ratio`)
    rejects({ kind: "IssuerRepurchase", value: { plan: { ...repurchasePlan, max_shares: invalid } } }, `${path}.value.plan.max_shares`)
    rejects({ kind: "StockDistribution", value: { ...common, kind: "BonusShares", shares_per_existing_share_micros: "100000", approved_total_new_shares: invalid } }, `${path}.value.approved_total_new_shares`)
  }
})

test("金额使用整数分字符串，拒绝小数价格与数字金额", { timeout: 10000 }, () => {
  for (const invalid of [1, "1.5", "1.00", "01", "9223372036854775808"]) {
    rejects({ kind: "RightsOffering", value: { plan: { ...rightsPlan, price_per_share: invalid } } }, `${path}.value.plan.price_per_share`)
    rejects({ kind: "CashDividend", value: { plan: cashPlan, total_gross: invalid } }, `${path}.value.total_gross`)
    rejects({ kind: "IssuerRepurchase", value: { plan: { ...repurchasePlan, price_cap_per_share: invalid } } }, `${path}.value.plan.price_cap_per_share`)
  }
})

test("CompanyId 使用 u32 number，AccountId 保持独立的 u64 string", { timeout: 10000 }, () => {
  const mode = { DirectedPlacement: { targets: [
    { NamedHolder: { holder: { Account: "18446744073709551615" }, shares: "100", lock_until: null } },
    { NamedHolder: { holder: { External: "outside" }, shares: "100", lock_until: "2030-02-01" } },
    { NamedHolder: { holder: "IssuerTreasury", shares: "100", lock_until: null } },
    { OpenPublicSubscription: { shares: "100" } },
  ] } }
  const fixture = { kind: "RightsOffering", value: { plan: { ...rightsPlan, mode } } }
  assert.deepEqual(parseAnnouncementContent(fixture, path), fixture)
  for (const issuer of ["0", -1, 4_294_967_296, 0.5]) {
    rejects({ kind: "ShareSplit", value: { ...common, issuer, direction: "Split", ratio: "2" } }, `${path}.value.issuer`)
  }
  for (const account of [1, "01", "18446744073709551616"]) {
    const invalidMode = { DirectedPlacement: { targets: [{ NamedHolder: { holder: { Account: account }, shares: "100", lock_until: null } }] } }
    rejects({ ...fixture, value: { plan: { ...rightsPlan, mode: invalidMode } } }, `${path}.value.plan.mode.DirectedPlacement.targets[0].NamedHolder.holder.Account`)
  }
})

test("Shock 复用既有冲击解析且 amplitude_bp 限于 i32", { timeout: 10000 }, () => {
  const fixture = { kind: "Shock", value: { kind: { PaymentFailure: { what: "tax", amount: "1.00", obligation_status: "StatutoryPaymentFailure" } }, amplitude_bp: 0, starts_on: "2030-01-01", expires_on: "2030-01-01" } }
  assert.deepEqual(parseAnnouncementContent(fixture, path), fixture)
  rejects({ ...fixture, value: { ...fixture.value, amplitude_bp: 2_147_483_648 } }, `${path}.value.amplitude_bp`)
})


test("方案枚举逐一保留已声明变体，nullable 锚不补默认值", { timeout: 10000 }, () => {
  for (const purpose of ["ReduceCapital", "EmployeeIncentive", "ConvertibleConversion", "ValueMaintenance"]) {
    const fixture = { kind: "IssuerRepurchase", value: { plan: { ...repurchasePlan, purpose, approval_unit_event_id: "split-1" } } }
    assert.deepEqual(parseAnnouncementContent(fixture, path), fixture)
  }
  const cash = { kind: "CashDividend", value: { plan: { ...cashPlan, formula: "ExchangeApprovedAdjustment" }, total_gross: "1" } }
  assert.deepEqual(parseAnnouncementContent(cash, path), cash)
  const strategy = { kind: "RightsOffering", value: { plan: { ...rightsPlan, npc_subscription_strategy: "StrategyBased" } } }
  // StrategyBased 是已声明的 wire 值；engine 受理时显式拒绝未实现行为。
  assert.deepEqual(parseAnnouncementContent(strategy, path), strategy)
  rejects({ kind: "IssuerRepurchase", value: { plan: { ...repurchasePlan, purpose: "Unknown" } } }, `${path}.value.plan.purpose`)
  rejects({ kind: "IssuerRepurchase", value: { plan: { ...repurchasePlan, completion_policy: "Unknown" } } }, `${path}.value.plan.completion_policy`)
  rejects({ kind: "ShareSplit", value: { ...common, exchange: "Unknown", direction: "Split", ratio: "2" } }, `${path}.value.exchange`)
  rejects({ kind: "ShareSplit", value: { ...common, direction: "Unknown", ratio: "2" } }, `${path}.value.direction`)
  rejects({ kind: "RightsOffering", value: { plan: { ...rightsPlan, mode: { Unknown: {} } } } }, `${path}.value.plan.mode`)
})

test("定向方案递归拒绝未知、缺失和非规范字段", { timeout: 10000 }, () => {
  const target = { holder: { Account: "1" }, shares: "100", lock_until: null }
  const fixture = (mode: unknown) => ({ kind: "RightsOffering", value: { plan: { ...rightsPlan, mode } } })
  const targetPath = `${path}.value.plan.mode.DirectedPlacement.targets[0].NamedHolder`
  for (const field of Object.keys(target)) {
    const invalid: Record<string, unknown> = { ...target }
    delete invalid[field]
    rejects(fixture({ DirectedPlacement: { targets: [{ NamedHolder: invalid }] } }), `${targetPath}.${field}`)
  }
  rejects(fixture({ DirectedPlacement: { targets: [{ NamedHolder: { ...target, extra: 1 } }] } }), `${targetPath}.extra`)
  rejects(fixture({ DirectedPlacement: { targets: [{ NamedHolder: { ...target, holder: { Unknown: "1" } } }] } }), `${targetPath}.holder`)
  rejects(fixture({ DirectedPlacement: { targets: [{ NamedHolder: { ...target, holder: { Account: "1", External: "outside" } } }] } }), `${targetPath}.holder`)
  rejects(fixture({ DirectedPlacement: { targets: [{ Unknown: {} }] } }), `${path}.value.plan.mode.DirectedPlacement.targets[0]`)
  rejects(fixture({ DirectedPlacement: { targets: [{ OpenPublicSubscription: { shares: "01" } }] } }), `${path}.value.plan.mode.DirectedPlacement.targets[0].OpenPublicSubscription.shares`)
  rejects(fixture({ RightsToAllShareholders: { shares_per_existing_share_micros: 100000 } }), `${path}.value.plan.mode.RightsToAllShareholders.shares_per_existing_share_micros`)
})
