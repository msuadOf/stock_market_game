import assert from "node:assert/strict"
import test from "node:test"
import { parseSaveSlot } from "../save-schema.ts"
import { currentSaveFixture } from "../current-save-fixture.ts"

const policy = {
  policy_version: 1,
  drop_30min_threshold_bp: -450,
  drop_1min_threshold_bp: -100,
  urgent_drawdown_threshold_bp: 2500,
  patient_confidence_threshold_bp: 3500,
  urgent_remaining_trading_days: 2,
  resume_signal_threshold_bp: 2200,
}

test("save requires an explicit urgency policy without legacy defaults", () => {
  const save = currentSaveFixture()
  delete save.urgency_policy
  assert.throws(() => parseSaveSlot(save), /urgency_policy.*必填/)
})

test("save preserves the frozen non-default urgency policy", () => {
  const save = { ...currentSaveFixture(), urgency_policy: policy }
  assert.deepEqual(parseSaveSlot(save).urgency_policy, policy)
})

test("save urgency policy rejects missing, extra, unsupported and invalid fields", () => {
  for (const field of Object.keys(policy)) {
    const missing: Record<string, unknown> = { ...policy }
    delete missing[field]
    assert.throws(() => parseSaveSlot({ ...currentSaveFixture(), urgency_policy: missing }), new RegExp(`urgency_policy\\.${field}`))
  }
  for (const [field, value] of [
    ["policy_version", 2],
    ["drop_30min_threshold_bp", 0],
    ["drop_30min_threshold_bp", -2_147_483_649],
    ["drop_1min_threshold_bp", 0],
    ["urgent_drawdown_threshold_bp", 10_001],
    ["patient_confidence_threshold_bp", 10_001],
    ["urgent_remaining_trading_days", 0],
    ["urgent_remaining_trading_days", 4_294_967_296],
    ["resume_signal_threshold_bp", -1],
    ["resume_signal_threshold_bp", 10_001],
    ["patient_confidence_threshold_bp", 0.5],
    ["extra", 1],
  ] as const) {
    assert.throws(() => parseSaveSlot({ ...currentSaveFixture(), urgency_policy: { ...policy, [field]: value } }), new RegExp(`urgency_policy\\.${field}`))
  }
})
