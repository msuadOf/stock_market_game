import assert from "node:assert/strict"
import test from "node:test"
import { validateDayEndArchive } from "./day-end-candidate.ts"
import type { StrictSaveEnvelope } from "./schema/root.ts"

test("公共日终入口拒绝活动分钟且不删除输入事实", { timeout: 10000 }, () => {
  const value = { runtime_state: { active_minute_history: { "600000": [{ minute_of_day: 570 }] } } } as unknown as StrictSaveEnvelope
  const before = structuredClone(value)
  assert.throws(() => validateDayEndArchive(value), /日内活动分钟/)
  assert.deepEqual(value, before)
})
