import { array, civilDate, exact, integer, oneOf, record, string } from "./primitives.ts"

type CivilClock = {
  readonly current_date: string
  readonly settled_through: string | null
  readonly next_due_seq: number
  readonly pending_due: readonly { readonly id: number; readonly due_date: string; readonly kind: "InterestAccrual" | "ContractMaturity" }[]
  readonly policy: CalendarPolicy
}

type CalendarPolicy = {
  readonly algorithm_version: number
  readonly default_start: string
  readonly runtime_min_start: string
  readonly runtime_max_end: string
  readonly init_only_min_start: string
  readonly official_coverage: readonly unknown[]
  readonly simulated_fallback: unknown
}

export function parseCivilClock(value: unknown, path = "civil_clock"): CivilClock {
  const parsed = record(value, path)
  exact(parsed, ["current_date", "settled_through", "next_due_seq", "pending_due", "policy"], path)
  return {
    current_date: civilDate(parsed.current_date, `${path}.current_date`),
    settled_through: parsed.settled_through === null ? null : civilDate(parsed.settled_through, `${path}.settled_through`),
    next_due_seq: integer(parsed.next_due_seq, `${path}.next_due_seq`, 0),
    pending_due: array(parsed.pending_due, `${path}.pending_due`).map((entry, index) => parseDue(entry, `${path}.pending_due[${index}]`)),
    policy: parsePolicy(parsed.policy, `${path}.policy`),
  }
}

function parseDue(value: unknown, path: string): CivilClock["pending_due"][number] {
  const parsed = record(value, path)
  exact(parsed, ["id", "due_date", "kind"], path)
  return { id: integer(parsed.id, `${path}.id`, 0), due_date: civilDate(parsed.due_date, `${path}.due_date`), kind: oneOf(parsed.kind, `${path}.kind`, ["InterestAccrual", "ContractMaturity"] as const) }
}

function parsePolicy(value: unknown, path: string): CalendarPolicy {
  const parsed = record(value, path)
  exact(parsed, ["algorithm_version", "default_start", "runtime_min_start", "runtime_max_end", "init_only_min_start", "official_coverage", "simulated_fallback"], path)
  return {
    algorithm_version: integer(parsed.algorithm_version, `${path}.algorithm_version`, 0),
    default_start: civilDate(parsed.default_start, `${path}.default_start`),
    runtime_min_start: civilDate(parsed.runtime_min_start, `${path}.runtime_min_start`),
    runtime_max_end: civilDate(parsed.runtime_max_end, `${path}.runtime_max_end`),
    init_only_min_start: civilDate(parsed.init_only_min_start, `${path}.init_only_min_start`),
    official_coverage: array(parsed.official_coverage, `${path}.official_coverage`).map((entry, index) => parseCoverage(entry, `${path}.official_coverage[${index}]`)),
    simulated_fallback: parseFallback(parsed.simulated_fallback, `${path}.simulated_fallback`),
  }
}

function parseCoverage(value: unknown, path: string): unknown {
  const parsed = record(value, path)
  exact(parsed, ["exchange", "year", "closed_ranges", "source_citation_id", "source_digest"], path)
  oneOf(parsed.exchange, `${path}.exchange`, ["sse", "szse"] as const)
  integer(parsed.year, `${path}.year`)
  array(parsed.closed_ranges, `${path}.closed_ranges`).forEach((entry, index) => { const range = record(entry, `${path}.closed_ranges[${index}]`); exact(range, ["from", "to"], `${path}.closed_ranges[${index}]`); civilDate(range.from, `${path}.closed_ranges[${index}].from`); civilDate(range.to, `${path}.closed_ranges[${index}].to`) })
  string(parsed.source_citation_id, `${path}.source_citation_id`)
  string(parsed.source_digest, `${path}.source_digest`)
  return parsed
}

function parseFallback(value: unknown, path: string): unknown {
  const parsed = record(value, path)
  exact(parsed, ["version", "notice_unverified_year", "lunar_facts", "digest"], path)
  integer(parsed.version, `${path}.version`, 0)
  integer(parsed.notice_unverified_year, `${path}.notice_unverified_year`)
  const lunar = record(parsed.lunar_facts, `${path}.lunar_facts`)
  exact(lunar, ["facts", "digest"], `${path}.lunar_facts`)
  array(lunar.facts, `${path}.lunar_facts.facts`).forEach((entry, index) => { const fact = record(entry, `${path}.lunar_facts.facts[${index}]`); exact(fact, ["year", "lunar_new_year", "dragon_boat", "mid_autumn", "qingming"], `${path}.lunar_facts.facts[${index}]`); integer(fact.year, `${path}.lunar_facts.facts[${index}].year`); for (const key of ["lunar_new_year", "dragon_boat", "mid_autumn", "qingming"] as const) civilDate(fact[key], `${path}.lunar_facts.facts[${index}].${key}`) })
  string(lunar.digest, `${path}.lunar_facts.digest`)
  string(parsed.digest, `${path}.digest`)
  return parsed
}
