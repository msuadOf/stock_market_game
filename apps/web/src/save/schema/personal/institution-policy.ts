import { exact, integer, oneOf, record, SaveSchemaError } from "../primitives.ts"

export type InstitutionLossResponse = "HoldOrAdd" | "PauseAndReview"

export type InstitutionExperiencePolicy = {
  readonly policy_version: 1
  readonly loss_response: InstitutionLossResponse
  readonly cost_loss_threshold_bp: number
  readonly cost_profit_threshold_bp: number
  readonly risk_pause_drawdown_bp: number
  readonly risk_resume_drawdown_bp: number
  readonly risk_pause_failed_buys: number
  readonly adverse_move_threshold_bp: number
}

const LOSS_RESPONSES = ["HoldOrAdd", "PauseAndReview"] as const
const FIELDS = [
  "policy_version",
  "loss_response",
  "cost_loss_threshold_bp",
  "cost_profit_threshold_bp",
  "risk_pause_drawdown_bp",
  "risk_resume_drawdown_bp",
  "risk_pause_failed_buys",
  "adverse_move_threshold_bp",
] as const

export function parseInstitutionExperiencePolicy(value: unknown): InstitutionExperiencePolicy {
  const path = "institution_experience_policy"
  const parsed = record(value, path)
  exact(parsed, FIELDS, path)

  function bounded(field: string, minimum: number, maximum: number): number {
    const result = integer(parsed[field], `${path}.${field}`, minimum)
    if (result > maximum) throw new SaveSchemaError(`${path}.${field}`, `必须不大于 ${maximum}`)
    return result
  }

  const policyVersion = bounded("policy_version", 1, 1)
  const riskPauseDrawdownBp = bounded("risk_pause_drawdown_bp", 1, 10_000)
  const riskResumeDrawdownBp = bounded("risk_resume_drawdown_bp", 1, 10_000)
  if (riskResumeDrawdownBp >= riskPauseDrawdownBp) {
    throw new SaveSchemaError(
      `${path}.risk_resume_drawdown_bp`,
      "必须小于 risk_pause_drawdown_bp",
    )
  }
  return {
    policy_version: policyVersion as 1,
    loss_response: oneOf(parsed.loss_response, `${path}.loss_response`, LOSS_RESPONSES),
    cost_loss_threshold_bp: bounded("cost_loss_threshold_bp", 1, 10_000),
    cost_profit_threshold_bp: bounded("cost_profit_threshold_bp", 1, 10_000),
    risk_pause_drawdown_bp: riskPauseDrawdownBp,
    risk_resume_drawdown_bp: riskResumeDrawdownBp,
    risk_pause_failed_buys: bounded("risk_pause_failed_buys", 1, 4_294_967_295),
    adverse_move_threshold_bp: bounded("adverse_move_threshold_bp", 1, 10_000),
  }
}
