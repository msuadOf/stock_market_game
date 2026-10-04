import type { NpcAttentionState } from "../../types/generated/NpcAttentionState"
import type { NpcInformationCadence } from "../../types/generated/NpcInformationCadence"
import { decimal, exact, integer, record, SaveSchemaError } from "./primitives.ts"
import { parseCivilInstant } from "./personal/experience.ts"

export function parseInformationCadence(value: unknown, path: string): NpcInformationCadence {
  if (value === "Immediate" || value === "Monthly") return value
  const cadence = record(value, path)
  const keys = Object.keys(cadence)
  if (keys.length !== 1) throw new SaveSchemaError(path, "关注节奏必须为单一变体")
  if (keys[0] === "Daily") {
    const item = record(cadence.Daily, `${path}.Daily`)
    exact(item, ["checks"], `${path}.Daily`)
    const checks = integer(item.checks, `${path}.Daily.checks`, 1)
    if (checks > 2) throw new SaveSchemaError(path, "每日信息检查只能为1或2次")
    return { Daily: { checks } }
  }
  if (keys[0] === "EveryDays") {
    const item = record(cadence.EveryDays, `${path}.EveryDays`)
    exact(item, ["days"], `${path}.EveryDays`)
    const days = integer(item.days, `${path}.EveryDays.days`, 1)
    if (days > 65535) throw new SaveSchemaError(path, "自然日间隔超出u16范围")
    return { EveryDays: { days } }
  }
  throw new SaveSchemaError(path, "无效信息关注节奏")
}

export function parseNpcAttention(value: unknown, path: string): NpcAttentionState {
  const attention = record(value, path)
  exact(attention, ["base_probability", "next_attention_candidate_tick", "rng_state", "information_cadence", "next_information_check"], path)
  const probability = attention.base_probability
  if (typeof probability !== "number" || !Number.isFinite(probability) || probability <= 0 || probability > 1) throw new SaveSchemaError(`${path}.base_probability`, "必须为有限(0,1]概率")
  return {
    base_probability: probability,
    next_attention_candidate_tick: decimal(attention.next_attention_candidate_tick, `${path}.next_attention_candidate_tick`),
    rng_state: decimal(attention.rng_state, `${path}.rng_state`),
    information_cadence: parseInformationCadence(attention.information_cadence, `${path}.information_cadence`),
    next_information_check: parseCivilInstant(attention.next_information_check, `${path}.next_information_check`),
  }
}
