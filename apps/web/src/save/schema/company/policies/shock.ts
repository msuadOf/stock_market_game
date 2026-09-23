import { SaveSchemaError, array, civilDate, exact, integer, oneOf, record, string } from "../../primitives.ts"

const unitShockKinds = ["MarketDemandShift", "CompanyDemandShift", "ContractWon", "ContractCancelled", "CreditDeterioration", "ProductionInterruption", "AssetImpairmentSignal"] as const
type UnitShockKind = (typeof unitShockKinds)[number]
export type ShockKind = UnitShockKind | { readonly IndustryCostShift: { readonly industry: string } }
export type ActiveShock = { readonly kind: ShockKind; readonly amplitude_bp: number; readonly starts_on: string; readonly expires_on: string }
export type ShockParams = { readonly version: number; readonly market_candidate_bp: number; readonly industry_candidate_bp: number; readonly company_candidate_bp: number; readonly duration_min_days: number; readonly duration_max_days: number; readonly market_demand_band_bp: number; readonly industry_cost_band_bp: number; readonly company_demand_band_bp: number; readonly credit_deterioration_add_bp: number }

export function parseShockKind(value: unknown, path: string): ShockKind {
  if (typeof value === "string") {
    return oneOf(value, path, unitShockKinds)
  }
  const entries = Object.entries(record(value, path))
  if (entries.length !== 1 || entries[0] === undefined || entries[0][0] !== "IndustryCostShift") throw new SaveSchemaError(path, "包含无效冲击种类")
  const item = record(entries[0][1], `${path}.IndustryCostShift`)
  exact(item, ["industry"], `${path}.IndustryCostShift`)
  return { IndustryCostShift: { industry: string(item.industry, `${path}.IndustryCostShift.industry`) } }
}

export function parseActiveShock(value: unknown, path: string): ActiveShock {
  const item = record(value, path)
  exact(item, ["kind", "amplitude_bp", "starts_on", "expires_on"], path)
  const startsOn = civilDate(item.starts_on, `${path}.starts_on`)
  const expiresOn = civilDate(item.expires_on, `${path}.expires_on`)
  if (expiresOn < startsOn) throw new SaveSchemaError(`${path}.expires_on`, "不得早于 starts_on")
  return { kind: parseShockKind(item.kind, `${path}.kind`), amplitude_bp: integer(item.amplitude_bp, `${path}.amplitude_bp`), starts_on: startsOn, expires_on: expiresOn }
}

export function parseActiveShocks(value: unknown, path: string): readonly ActiveShock[] {
  return array(value, path).map((entry, index) => parseActiveShock(entry, `${path}[${index}]`))
}

export function parseShockParams(value: unknown, path: string): ShockParams {
  const item = record(value, path)
  exact(item, ["version", "market_candidate_bp", "industry_candidate_bp", "company_candidate_bp", "duration_min_days", "duration_max_days", "market_demand_band_bp", "industry_cost_band_bp", "company_demand_band_bp", "credit_deterioration_add_bp"], path)
  const marketCandidate = integer(item.market_candidate_bp, `${path}.market_candidate_bp`)
  const industryCandidate = integer(item.industry_candidate_bp, `${path}.industry_candidate_bp`)
  const companyCandidate = integer(item.company_candidate_bp, `${path}.company_candidate_bp`)
  const durationMin = integer(item.duration_min_days, `${path}.duration_min_days`)
  const durationMax = integer(item.duration_max_days, `${path}.duration_max_days`)
  const marketBand = integer(item.market_demand_band_bp, `${path}.market_demand_band_bp`)
  const industryBand = integer(item.industry_cost_band_bp, `${path}.industry_cost_band_bp`)
  const companyBand = integer(item.company_demand_band_bp, `${path}.company_demand_band_bp`)
  const creditAddition = integer(item.credit_deterioration_add_bp, `${path}.credit_deterioration_add_bp`)
  if ([marketCandidate, industryCandidate, companyCandidate].some((candidate) => candidate < 0 || candidate > 10_000)) throw new SaveSchemaError(path, "候选概率必须在 0 至 10000 之间")
  if ([marketBand, industryBand, companyBand].some((band) => band <= 0)) throw new SaveSchemaError(path, "冲击幅度带必须为正数")
  if (creditAddition < 0) throw new SaveSchemaError(`${path}.credit_deterioration_add_bp`, "必须为非负数")
  if (durationMin < 1 || durationMax < durationMin) throw new SaveSchemaError(path, "持续期范围无效")
  return { version: integer(item.version, `${path}.version`), market_candidate_bp: marketCandidate, industry_candidate_bp: industryCandidate, company_candidate_bp: companyCandidate, duration_min_days: durationMin, duration_max_days: durationMax, market_demand_band_bp: marketBand, industry_cost_band_bp: industryBand, company_demand_band_bp: companyBand, credit_deterioration_add_bp: creditAddition }
}
