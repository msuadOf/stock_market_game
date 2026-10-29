import assert from "node:assert/strict"
import test from "node:test"
import { DEFAULT_SETUP } from "../config/defaults.ts"
import { currentSaveFixture } from "./current-save-fixture.ts"
import { parseSaveSlot } from "./save-schema.ts"
import { parseSetup } from "./schema/market.ts"

const policies = ["every_tick", "day_end", "on_demand"] as const

test("新局 Web 默认明确选择 every_tick", { timeout: 10_000 }, () => {
  assert.equal(DEFAULT_SETUP.runtime_sync_policy, "every_tick")
})

test("当前 SaveSlot 要求并原样保留三种 runtime_sync_policy", { timeout: 10_000 }, () => {
  for (const runtime_sync_policy of policies) {
    const save = currentSaveFixture()
    save.setup = { ...(save.setup as Record<string, unknown>), runtime_sync_policy }
    assert.equal(parseSaveSlot(save).setup.runtime_sync_policy, runtime_sync_policy)
    assert.equal(parseSetup(save.setup, "setup").runtime_sync_policy, runtime_sync_policy)
  }
})

test("当前 SaveSlot 缺少或携带未知 runtime_sync_policy 时显式拒绝", { timeout: 10_000 }, () => {
  const save = currentSaveFixture()
  const setup = save.setup as Record<string, unknown>
  delete setup.runtime_sync_policy
  assert.throws(() => parseSaveSlot(save), /setup\.runtime_sync_policy/)

  for (const runtime_sync_policy of ["", "EveryTick", "future_policy", null, 1]) {
    assert.throws(() => parseSetup({ ...setup, runtime_sync_policy }, "setup"), /runtime_sync_policy/)
    assert.throws(() => parseSaveSlot({
      ...currentSaveFixture(),
      setup: { ...(currentSaveFixture().setup as Record<string, unknown>), runtime_sync_policy },
    }), /runtime_sync_policy/)
  }
})
