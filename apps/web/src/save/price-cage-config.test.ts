import assert from "node:assert/strict"
import test from "node:test"
import { DEFAULT_SETUP } from "../config/defaults.ts"
import { parseSetup } from "./schema/market.ts"

test("new games enable the price cage by default", () => {
  assert.equal(DEFAULT_SETUP.config.price_cage_enabled, true)
})

test("save setup preserves an explicitly enabled or disabled price cage", () => {
  for (const enabled of [true, false]) {
    const setup = {
      ...DEFAULT_SETUP,
      config: { ...DEFAULT_SETUP.config, price_cage_enabled: enabled },
    }
    assert.deepEqual(parseSetup(setup, "setup"), setup)
  }
})

test("save setup requires an explicit boolean price cage choice", () => {
  const config: Record<string, unknown> = { ...DEFAULT_SETUP.config }
  delete config.price_cage_enabled
  assert.throws(() => parseSetup({ ...DEFAULT_SETUP, config }, "setup"), /config\.price_cage_enabled.*必填/)
  for (const value of [null, 0, 1, "false"]) {
    assert.throws(
      () => parseSetup({ ...DEFAULT_SETUP, config: { ...config, price_cage_enabled: value } }, "setup"),
      /config\.price_cage_enabled.*布尔/,
    )
  }
})
