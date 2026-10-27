import { parsePlanU64 } from "../../../utils/plan-identity.ts"
import { array, exact, map, record } from "../primitives.ts"
import { accountKey, companyKey, publicationId, type StringMap } from "./common.ts"
import { parseCivilInstant } from "./experience.ts"

export type AcquisitionRecord = {
  readonly id: number
  readonly observed_at: CivilInstant
  readonly kind: "Report" | "Announcement"
}

export type CivilInstant = {
  readonly date: string
  readonly second_of_day: number
}

export type InformationState = {
  readonly owner: string
  readonly companies: StringMap<readonly AcquisitionRecord[]>
}

export function parseInformationStates(value: unknown, path = "information_states"): StringMap<InformationState> {
  const result = map(value, path, accountKey, parseInformationState)
  for (const [owner, state] of Object.entries(result)) if (state.owner !== owner) throw new Error(`存档 ${path}.${owner}.owner 必须与账户 key 一致`)
  return result
}

export function parseInformationState(value: unknown, path: string): InformationState {
  const parsed = record(value, path)
  exact(parsed, ["owner", "companies"], path)
  return {
    owner: parsePlanU64(parsed.owner, `${path}.owner`),
    companies: map(parsed.companies, `${path}.companies`, companyKey, parseAcquisitionRecords),
  }
}

function parseAcquisitionRecords(value: unknown, path: string): readonly AcquisitionRecord[] {
  return array(value, path).map((entry, index) => parseAcquisitionRecord(entry, `${path}[${index}]`))
}

function parseAcquisitionRecord(value: unknown, path: string): AcquisitionRecord {
  const parsed = record(value, path)
  exact(parsed, ["id", "observed_at", "kind"], path)
  const kind = parsed.kind
  if (kind !== "Report" && kind !== "Announcement") throw new Error(`存档 ${path}.kind 包含无效枚举值`)
  return { id: publicationId(parsed.id, `${path}.id`), observed_at: parseCivilInstant(parsed.observed_at, `${path}.observed_at`), kind }
}
