import type { UrgencyPolicy } from "../../types/generated/UrgencyPolicy"
import { exact, integer, record, SaveSchemaError } from "./primitives.ts"

export function parseUrgencyPolicy(value: unknown): UrgencyPolicy {
  const path = "urgency_policy"
  const policy = record(value, path)
  exact(policy, ["policy_version", "drop_30min_threshold_bp", "drop_1min_threshold_bp", "urgent_drawdown_threshold_bp", "patient_confidence_threshold_bp", "urgent_remaining_trading_days", "resume_signal_threshold_bp"], path)
  function bounded(field: string, minimum: number, maximum: number): number {
    const parsed = integer(policy[field], `${path}.${field}`, minimum)
    if (parsed > maximum) throw new SaveSchemaError(`${path}.${field}`, `必须不大于 ${maximum}`)
    return parsed
  }
  return {
    policy_version: bounded("policy_version", 1, 1),
    drop_30min_threshold_bp: bounded("drop_30min_threshold_bp", -2_147_483_648, -1),
    drop_1min_threshold_bp: bounded("drop_1min_threshold_bp", -2_147_483_648, -1),
    urgent_drawdown_threshold_bp: bounded("urgent_drawdown_threshold_bp", 0, 10_000),
    patient_confidence_threshold_bp: bounded("patient_confidence_threshold_bp", 0, 10_000),
    urgent_remaining_trading_days: bounded("urgent_remaining_trading_days", 1, 4_294_967_295),
    resume_signal_threshold_bp: bounded("resume_signal_threshold_bp", 0, 10_000),
  }
}
