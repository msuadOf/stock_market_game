import assert from "node:assert/strict"
import test from "node:test"
import { DEFAULT_SETUP } from "../config/defaults.ts"
import { parseSetup } from "./schema/market.ts"

// 三层税制（2026-10-08 用户产品决策）：简税默认 + 大 A 可选 + 不扣税。
// 覆盖 parseSetup 的严格三态契约：仅 Flat 模式必填简税比例（0..=10000bp）、
// 其他模式显式拒绝携带；旧两变体枚举值拒绝；默认新局为简税 10%。

test("新游戏默认税务模式为简税并携带默认 10% 比例", () => {
  assert.equal(DEFAULT_SETUP.dividend_tax_mode, "FlatWithholding")
  assert.equal(DEFAULT_SETUP.flat_withholding_bp, 1000)
  const parsed = parseSetup(DEFAULT_SETUP, "setup")
  assert.equal(parsed.dividend_tax_mode, "FlatWithholding")
  assert.equal(parsed.flat_withholding_bp, 1000)
})

test("大 A 方式与不扣税模式的比例必须为 null", () => {
  for (const mode of ["AShareIndividual", "Exempt"] as const) {
    const setup = { ...DEFAULT_SETUP, dividend_tax_mode: mode, flat_withholding_bp: null }
    assert.equal(parseSetup(setup, "setup").dividend_tax_mode, mode)
  }
})

test("简税比例可编辑并保持合法域往返", () => {
  for (const rate of [0, 1, 5000, 10000]) {
    const setup = { ...DEFAULT_SETUP, flat_withholding_bp: rate }
    assert.equal(parseSetup(setup, "setup").flat_withholding_bp, rate)
  }
})

test("Flat 模式缺比例、非 Flat 模式携带比例都被显式拒绝", () => {
  assert.throws(
    () => parseSetup({ ...DEFAULT_SETUP, flat_withholding_bp: null }, "setup"),
    /FlatWithholding 模式必须携带简税比例/,
  )
  for (const mode of ["AShareIndividual", "Exempt"] as const) {
    assert.throws(
      () => parseSetup({ ...DEFAULT_SETUP, dividend_tax_mode: mode, flat_withholding_bp: 1000 }, "setup"),
      /仅 FlatWithholding 模式可携带/,
    )
  }
})

test("简税比例越界或非整数被显式拒绝", () => {
  for (const rate of [10001, 1_000_000]) {
    assert.throws(
      () => parseSetup({ ...DEFAULT_SETUP, flat_withholding_bp: rate }, "setup"),
      /超出合法域 0\.\.=10000bp/,
    )
  }
  for (const rate of ["1000", 10.5, null]) {
    assert.throws(
      () => parseSetup({ ...DEFAULT_SETUP, flat_withholding_bp: rate }, "setup"),
      /flat_withholding_bp/,
    )
  }
})

test("旧两变体枚举值被显式拒绝，不静默映射", () => {
  for (const legacy of ["IndividualPublicMarket", "Simple"]) {
    assert.throws(
      () => parseSetup({ ...DEFAULT_SETUP, dividend_tax_mode: legacy }, "setup"),
      /dividend_tax_mode/,
    )
  }
})

test("存档 setup 缺少比例字段时按模式三态判定", () => {
  // 非 Flat 模式的旧形态（无该键）等价 null，可解析；Flat 模式缺键即拒绝。
  const withoutKey: Record<string, unknown> = { ...DEFAULT_SETUP, dividend_tax_mode: "AShareIndividual" }
  delete withoutKey.flat_withholding_bp
  assert.equal(parseSetup(withoutKey, "setup").flat_withholding_bp, null)
  const flatWithoutKey: Record<string, unknown> = { ...DEFAULT_SETUP }
  delete flatWithoutKey.flat_withholding_bp
  assert.throws(
    () => parseSetup(flatWithoutKey, "setup"),
    /FlatWithholding 模式必须携带简税比例/,
  )
})
