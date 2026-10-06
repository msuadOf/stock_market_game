import assert from "node:assert/strict"
import test from "node:test"
import { DEFAULT_SETUP } from "../config/defaults.ts"
import { parseSetup } from "./schema/market.ts"

// 公司行为机制开关（2026-10-07 产品决策）：两个独立必填布尔，默认关闭；
// 旧档缺失字段显式拒绝（对齐 dividend_tax_mode 严格持久化先例）。

test("new games default both company mechanism switches to disabled", () => {
  assert.equal(DEFAULT_SETUP.rights_offering_enabled, false)
  assert.equal(DEFAULT_SETUP.issuer_repurchase_enabled, false)
})

test("save setup preserves explicit mechanism switch choices", () => {
  for (const [rights, repurchase] of [
    [true, false],
    [false, true],
    [true, true],
    [false, false],
  ] as const) {
    const setup = {
      ...DEFAULT_SETUP,
      rights_offering_enabled: rights,
      issuer_repurchase_enabled: repurchase,
    }
    assert.deepEqual(parseSetup(setup, "setup"), setup)
  }
})

test("save setup rejects a missing or non-boolean mechanism switch", () => {
  for (const field of ["rights_offering_enabled", "issuer_repurchase_enabled"] as const) {
    const missing: Record<string, unknown> = { ...DEFAULT_SETUP }
    delete missing[field]
    assert.throws(() => parseSetup(missing, "setup"), new RegExp(`${field}.*必填`))
    for (const value of [null, 0, 1, "false"]) {
      assert.throws(
        () => parseSetup({ ...DEFAULT_SETUP, [field]: value }, "setup"),
        new RegExp(`${field}.*布尔`),
      )
    }
  }
})
