import assert from "node:assert/strict"
import test from "node:test"
import { parseInstitutionExperiencePolicy } from "./institution-policy.ts"

const policy = {
  policy_version: 1,
  loss_response: "HoldOrAdd",
  cost_loss_threshold_bp: 700,
  cost_profit_threshold_bp: 1400,
  risk_pause_drawdown_bp: 2400,
  risk_resume_drawdown_bp: 1200,
  risk_pause_failed_buys: 2,
  adverse_move_threshold_bp: 600,
}

test("institution experience policy preserves its exact eight fields", () => {
  assert.deepEqual(parseInstitutionExperiencePolicy(policy), policy)
})

test("institution experience policy rejects missing or additional fields", () => {
  for (const field of Object.keys(policy)) {
    const missing: Record<string, unknown> = { ...policy }
    delete missing[field]
    assert.throws(() => parseInstitutionExperiencePolicy(missing), new RegExp(`institution_experience_policy\\.${field}`))
  }
  assert.throws(() => parseInstitutionExperiencePolicy({ ...policy, legacy_default: 1 }), /legacy_default/)
})

test("institution experience policy rejects invalid values and threshold relationships", () => {
  const invalidCases: readonly (readonly [string, unknown])[] = [
    ["policy_version", 2],
    ["loss_response", "Sell"],
    ["cost_loss_threshold_bp", 0],
    ["cost_loss_threshold_bp", -1],
    ["cost_loss_threshold_bp", 10_001],
    ["cost_profit_threshold_bp", 10_001],
    ["risk_pause_drawdown_bp", 0],
    ["risk_pause_drawdown_bp", 10_001],
    ["risk_resume_drawdown_bp", 0],
    ["risk_pause_failed_buys", 0],
    ["adverse_move_threshold_bp", 10_001],
  ]
  for (const [field, value] of invalidCases) {
    const invalid = { ...policy, [field]: value }
    assert.throws(() => parseInstitutionExperiencePolicy(invalid))
  }
  for (const [pause, resume] of [[1200, 1200], [1200, 1300]]) {
    assert.throws(() => parseInstitutionExperiencePolicy({ ...policy, risk_pause_drawdown_bp: pause, risk_resume_drawdown_bp: resume }))
  }
})
