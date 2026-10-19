import assert from "node:assert/strict"
import test from "node:test"
import { validateDayEndArchive } from "./day-end-candidate.ts"
import { withSettledDay, currentSaveFixture } from "./current-save-fixture.ts"
import type { StrictSaveEnvelope } from "./schema/root.ts"

// B2 保存域（W3 接入）：日结清空当日分钟桶（bars 冻结进保留链——引擎
// clear_runtime_minute_history）。桶非空的档是日内档，公共日终入口拒绝。
test("公共日终入口拒绝活动分钟且不删除输入事实", { timeout: 10000 }, () => {
  const value = withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02") as unknown as StrictSaveEnvelope
  const slot = { ...value, market_minute_bars: { "0": [{ minute_of_day: 570 }] } } as unknown as StrictSaveEnvelope
  const before = structuredClone(slot)
  assert.throws(() => validateDayEndArchive(slot), /日内活动分钟/)
  assert.deepEqual(slot, before)
})
