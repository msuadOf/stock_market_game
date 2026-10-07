import assert from "node:assert/strict"
import test from "node:test"
import { DEFAULT_SETUP } from "../config/defaults.ts"
import { parseSetup } from "./schema/market.ts"

// 面值与开局自动装配（2026-10-08 N2a 用户决策）：面值默认 1 元/股（100 分）、
// 新局可编辑；开局自动装配名册+法定事实默认开启。严格持久化：旧档缺失
// 字段或面值非正显式拒绝（对齐 dividend_tax_mode / 机制开关先例）。

test("new games default to one yuan par value and enabled auto foundation", () => {
  assert.equal(DEFAULT_SETUP.par_value_per_share, "100")
  assert.equal(DEFAULT_SETUP.auto_corporate_foundation, true)
})

test("save setup preserves explicit par value and foundation choices", () => {
  for (const [par, foundation] of [
    ["100", true],
    ["200", true],
    ["100", false],
  ] as const) {
    const setup = { ...DEFAULT_SETUP, par_value_per_share: par, auto_corporate_foundation: foundation }
    assert.deepEqual(parseSetup(setup, "setup"), setup)
  }
})

test("save setup rejects a missing par value or foundation flag", () => {
  for (const field of ["par_value_per_share", "auto_corporate_foundation"] as const) {
    const missing: Record<string, unknown> = { ...DEFAULT_SETUP }
    delete missing[field]
    assert.throws(() => parseSetup(missing, "setup"), (error: unknown) => {
      assert.ok(error instanceof Error)
      return error.message.includes(field)
    }, `缺少 ${field} 必须显式拒绝`)
  }
})

test("save setup rejects a non-positive or malformed par value", () => {
  for (const value of ["0", "-100", null, 100, "1.5", "abc"]) {
    assert.throws(
      () => parseSetup({ ...DEFAULT_SETUP, par_value_per_share: value }, "setup"),
      (error: unknown) => {
        assert.ok(error instanceof Error)
        return /面值|par_value_per_share/.test(error.message)
      },
      `面值 ${JSON.stringify(value)} 必须被显式拒绝`,
    )
  }
})

test("new games carry mild default cash dividend preferences aligned with the input preset", () => {
  assert.equal(DEFAULT_SETUP.company_system.mode, "Simple")
  const { config } = DEFAULT_SETUP.company_system
  if (!("companies" in config)) throw new Error("DEFAULT_SETUP 必须是 Simple 配置")
  for (const company of config.companies) {
    assert.deepEqual(company.preferences.cash_dividend, {
      target_payout_bp: 3000,
      min_distributable_profit: "100000000",
      cycles_between_proposals: 1,
    }, `公司 ${company.company} 必须携带温和默认现金分红偏好`)
    assert.equal(company.preferences.stock_distribution, null, "温和默认送转偏好为关闭")
  }
})
