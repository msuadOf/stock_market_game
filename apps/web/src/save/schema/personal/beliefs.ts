import { array, exact, integer, map, nullable, oneOf, record, string } from "../primitives.ts"
import { accountKey, companyKey, externalTag, i32, money, orderId, publicationId, stockKey, tagged, u16, type StringMap } from "./common.ts"

export type BeliefBook = { readonly npc: number; readonly profile: StrategyProfile; readonly analysis: AnalysisProfile; readonly assumptions: PersonalAssumptions; readonly entries: StringMap<BeliefEntry> }
export type StrategyProfile = { readonly Retail: RetailStyle } | { readonly Institution: InstitutionStyle } | { readonly Hot: HotStyle }
export type RetailStyle = "Dormant" | "LongTerm" | "Noise" | "DipBuyer" | "Momentum" | "Panic"
export type InstitutionStyle = "DeepValue" | "Growth" | "Balanced" | "Defensive" | "ActiveTrader"
export type HotStyle = "Momentum" | "Reversal"
export type AnalysisProfile = { readonly fundamental_bp: number; readonly trend_bp: number; readonly price_volume_bp: number; readonly technical_bp: number; readonly experience_cost_bp: number; readonly fundamental_method: FundamentalMethod | null }
export type FundamentalMethod = "earnings_multiple" | "cash_flow" | "equity_roe"
export type PersonalAssumptions = { readonly growth_deviation_bp: number; readonly quality_coefficient_bp: number; readonly pe_multiple: number; readonly equity_cost_bp: number; readonly terminal_growth_bp: number; readonly roe_deviation_bp: number }
export type BeliefEntry = { readonly company: string; readonly method: FundamentalMethod | null; readonly forecast: ForecastState; readonly confidence_bp: number; readonly valuation: ValuationOutcome; readonly used_report_ids: readonly number[]; readonly anchor_trading_day: number; readonly horizon_trading_days: number; readonly last_cause: CauseRecord | null; readonly applied_experience_orders: readonly number[] }
export type ForecastState = { readonly growth_bp: number | null; readonly basis: ForecastBasis }
export type ForecastBasis = { readonly InitialTwoYear: { readonly observed_bp: number } } | "InitialWithoutHistory" | { readonly Revised: { readonly observed_bp: number } } | "Degenerate"
export type ValuationOutcome = { readonly Available: { readonly total_equity_estimate: string; readonly per_share: PerShareRange } } | { readonly Unavailable: { readonly reason: ValuationUnavailable } }
export type PerShareRange = { readonly pessimistic: number; readonly optimistic: number }
export type ValuationUnavailable = "MethodDisabled" | { readonly CompanyMismatch: { readonly expected: string; readonly report: string } } | { readonly UnsupportedReportKind: { readonly kind: string } } | { readonly FutureDatedMaterial: { readonly report: number } } | "NonPositiveNetIncome" | { readonly NonPositivePe: { readonly pe: number } } | { readonly NonPositiveCost: { readonly cost_bp: number } } | { readonly TerminalGrowthNotBelowCost: { readonly terminal_bp: number; readonly cost_bp: number } } | "NonPositiveBookEquity" | "NonPositiveAverageEquity" | { readonly NonPositiveExpectedRoe: { readonly expected_bp: number } } | "NonPositiveValuationEstimate" | "GrowthPriorUnavailable" | "FinancingSplitUndeterminable" | "ZeroIssuedShares" | { readonly Overflow: { readonly step: string } } | "PerShareOutOfRange"
export type CauseRecord = { readonly cause: BeliefCause; readonly as_of_trading_day: number }
export type BeliefCause = { readonly NewMaterial: { readonly report: number } } | { readonly Correction: { readonly report: number } } | { readonly CreditDefault: { readonly announcement: number } } | "HorizonExpired" | { readonly ExperienceFailure: { readonly order: number } } | { readonly ProfitableExit: { readonly order: number } }

const RETAIL = ["Dormant", "LongTerm", "Noise", "DipBuyer", "Momentum", "Panic"] as const
const INSTITUTION = ["DeepValue", "Growth", "Balanced", "Defensive", "ActiveTrader"] as const
const HOT = ["Momentum", "Reversal"] as const
const METHODS = ["earnings_multiple", "cash_flow", "equity_roe"] as const
const UNAVAILABLE_UNITS = ["MethodDisabled", "NonPositiveNetIncome", "NonPositiveBookEquity", "NonPositiveAverageEquity", "NonPositiveValuationEstimate", "GrowthPriorUnavailable", "FinancingSplitUndeterminable", "ZeroIssuedShares", "PerShareOutOfRange"] as const

export function parseBeliefBooks(value: unknown, path = "belief_books"): StringMap<BeliefBook> {
  return map(value, path, accountKey, parseBeliefBook)
}

export function parseBeliefBook(value: unknown, path: string): BeliefBook {
  const parsed = record(value, path)
  exact(parsed, ["npc", "profile", "analysis", "assumptions", "entries"], path)
  return { npc: integer(parsed.npc, `${path}.npc`, 0), profile: parseProfile(parsed.profile, `${path}.profile`), analysis: parseAnalysis(parsed.analysis, `${path}.analysis`), assumptions: parseAssumptions(parsed.assumptions, `${path}.assumptions`), entries: map(parsed.entries, `${path}.entries`, stockKey, parseEntry) }
}

function parseProfile(value: unknown, path: string): StrategyProfile {
  const [tag, payload] = externalTag(value, path)
  switch (tag) {
    case "Retail": return { Retail: oneOf(payload, `${path}.Retail`, RETAIL) }
    case "Institution": return { Institution: oneOf(payload, `${path}.Institution`, INSTITUTION) }
    case "Hot": return { Hot: oneOf(payload, `${path}.Hot`, HOT) }
    default: throw new Error(`存档 ${path}.${tag} 不是已知策略档案`)
  }
}

function parseAnalysis(value: unknown, path: string): AnalysisProfile {
  const parsed = record(value, path)
  exact(parsed, ["fundamental_bp", "trend_bp", "price_volume_bp", "technical_bp", "experience_cost_bp", "fundamental_method"], path)
  return { fundamental_bp: i32(parsed.fundamental_bp, `${path}.fundamental_bp`), trend_bp: i32(parsed.trend_bp, `${path}.trend_bp`), price_volume_bp: i32(parsed.price_volume_bp, `${path}.price_volume_bp`), technical_bp: i32(parsed.technical_bp, `${path}.technical_bp`), experience_cost_bp: i32(parsed.experience_cost_bp, `${path}.experience_cost_bp`), fundamental_method: nullable(parsed.fundamental_method, `${path}.fundamental_method`, (nested, nestedPath) => oneOf(nested, nestedPath, METHODS)) }
}

function parseAssumptions(value: unknown, path: string): PersonalAssumptions {
  const parsed = record(value, path)
  exact(parsed, ["growth_deviation_bp", "quality_coefficient_bp", "pe_multiple", "equity_cost_bp", "terminal_growth_bp", "roe_deviation_bp"], path)
  return { growth_deviation_bp: i32(parsed.growth_deviation_bp, `${path}.growth_deviation_bp`), quality_coefficient_bp: i32(parsed.quality_coefficient_bp, `${path}.quality_coefficient_bp`), pe_multiple: i32(parsed.pe_multiple, `${path}.pe_multiple`), equity_cost_bp: i32(parsed.equity_cost_bp, `${path}.equity_cost_bp`), terminal_growth_bp: i32(parsed.terminal_growth_bp, `${path}.terminal_growth_bp`), roe_deviation_bp: i32(parsed.roe_deviation_bp, `${path}.roe_deviation_bp`) }
}

function parseEntry(value: unknown, path: string): BeliefEntry {
  const parsed = record(value, path)
  exact(parsed, ["company", "method", "forecast", "confidence_bp", "valuation", "used_report_ids", "anchor_trading_day", "horizon_trading_days", "last_cause", "applied_experience_orders"], path)
  const company = string(parsed.company, `${path}.company`); companyKey(company, `${path}.company`)
  return { company, method: nullable(parsed.method, `${path}.method`, (nested, nestedPath) => oneOf(nested, nestedPath, METHODS)), forecast: parseForecast(parsed.forecast, `${path}.forecast`), confidence_bp: u16(parsed.confidence_bp, `${path}.confidence_bp`), valuation: parseValuation(parsed.valuation, `${path}.valuation`), used_report_ids: array(parsed.used_report_ids, `${path}.used_report_ids`).map((id, index) => publicationId(id, `${path}.used_report_ids[${index}]`)), anchor_trading_day: integer(parsed.anchor_trading_day, `${path}.anchor_trading_day`, 0), horizon_trading_days: u16(parsed.horizon_trading_days, `${path}.horizon_trading_days`), last_cause: nullable(parsed.last_cause, `${path}.last_cause`, parseCauseRecord), applied_experience_orders: array(parsed.applied_experience_orders, `${path}.applied_experience_orders`).map((id, index) => integer(id, `${path}.applied_experience_orders[${index}]`, 0)) }
}

function parseForecast(value: unknown, path: string): ForecastState {
  const parsed = record(value, path); exact(parsed, ["growth_bp", "basis"], path)
  return { growth_bp: nullable(parsed.growth_bp, `${path}.growth_bp`, i32), basis: parseForecastBasis(parsed.basis, `${path}.basis`) }
}

function parseForecastBasis(value: unknown, path: string): ForecastBasis {
  if (value === "InitialWithoutHistory" || value === "Degenerate") return value
  const [tag, payload] = tagged(value, path)
  if (tag !== "InitialTwoYear" && tag !== "Revised") throw new Error(`存档 ${path}.${tag} 不是已知预测依据`)
  exact(payload, ["observed_bp"], `${path}.${tag}`)
  if (tag === "InitialTwoYear") return { InitialTwoYear: { observed_bp: integer(payload.observed_bp, `${path}.InitialTwoYear.observed_bp`) } }
  return { Revised: { observed_bp: integer(payload.observed_bp, `${path}.Revised.observed_bp`) } }
}

function parseValuation(value: unknown, path: string): ValuationOutcome {
  const [tag, payload] = tagged(value, path)
  if (tag === "Available") { exact(payload, ["total_equity_estimate", "per_share"], `${path}.Available`); return { Available: { total_equity_estimate: parseAccountingAmount(payload.total_equity_estimate, `${path}.Available.total_equity_estimate`), per_share: parseRange(payload.per_share, `${path}.Available.per_share`) } } }
  if (tag === "Unavailable") { exact(payload, ["reason"], `${path}.Unavailable`); return { Unavailable: { reason: parseUnavailable(payload.reason, `${path}.Unavailable.reason`) } } }
  throw new Error(`存档 ${path}.${tag} 不是已知估值结果`)
}

function parseRange(value: unknown, path: string): PerShareRange { const parsed = record(value, path); exact(parsed, ["pessimistic", "optimistic"], path); return { pessimistic: money(parsed.pessimistic, `${path}.pessimistic`), optimistic: money(parsed.optimistic, `${path}.optimistic`) } }
function parseAccountingAmount(value: unknown, path: string): string { const amount = string(value, path); if (!/^-?\d+\.\d{2}$/.test(amount)) throw new Error(`存档 ${path} 必须是两位小数的十进制会计金额`); return amount }

function parseUnavailable(value: unknown, path: string): ValuationUnavailable {
  if (typeof value === "string") {
    const unit = oneOf(value, path, UNAVAILABLE_UNITS)
    switch (unit) {
      case "MethodDisabled": return "MethodDisabled"
      case "NonPositiveNetIncome": return "NonPositiveNetIncome"
      case "NonPositiveBookEquity": return "NonPositiveBookEquity"
      case "NonPositiveAverageEquity": return "NonPositiveAverageEquity"
      case "NonPositiveValuationEstimate": return "NonPositiveValuationEstimate"
      case "GrowthPriorUnavailable": return "GrowthPriorUnavailable"
      case "FinancingSplitUndeterminable": return "FinancingSplitUndeterminable"
      case "ZeroIssuedShares": return "ZeroIssuedShares"
      case "PerShareOutOfRange": return "PerShareOutOfRange"
    }
  }
  const [tag, payload] = tagged(value, path)
  const fields: { readonly [key: string]: readonly string[] } = { CompanyMismatch: ["expected", "report"], UnsupportedReportKind: ["kind"], FutureDatedMaterial: ["report"], NonPositivePe: ["pe"], NonPositiveCost: ["cost_bp"], TerminalGrowthNotBelowCost: ["terminal_bp", "cost_bp"], NonPositiveExpectedRoe: ["expected_bp"], Overflow: ["step"] }
  const keys = fields[tag]; if (keys === undefined) throw new Error(`存档 ${path}.${tag} 包含无效估值不可用原因`); exact(payload, keys, `${path}.${tag}`)
  if (tag === "CompanyMismatch") return { CompanyMismatch: { expected: string(payload.expected, `${path}.${tag}.expected`), report: string(payload.report, `${path}.${tag}.report`) } }
  if (tag === "UnsupportedReportKind") return { UnsupportedReportKind: { kind: string(payload.kind, `${path}.${tag}.kind`) } }
  if (tag === "FutureDatedMaterial") return { FutureDatedMaterial: { report: publicationId(payload.report, `${path}.${tag}.report`) } }
  if (tag === "NonPositivePe") return { NonPositivePe: { pe: integer(payload.pe, `${path}.${tag}.pe`) } }
  if (tag === "NonPositiveCost") return { NonPositiveCost: { cost_bp: integer(payload.cost_bp, `${path}.${tag}.cost_bp`) } }
  if (tag === "TerminalGrowthNotBelowCost") return { TerminalGrowthNotBelowCost: { terminal_bp: integer(payload.terminal_bp, `${path}.${tag}.terminal_bp`), cost_bp: integer(payload.cost_bp, `${path}.${tag}.cost_bp`) } }
  if (tag === "NonPositiveExpectedRoe") return { NonPositiveExpectedRoe: { expected_bp: integer(payload.expected_bp, `${path}.${tag}.expected_bp`) } }
  return { Overflow: { step: string(payload.step, `${path}.${tag}.step`) } }
}

function parseCauseRecord(value: unknown, path: string): CauseRecord { const parsed = record(value, path); exact(parsed, ["cause", "as_of_trading_day"], path); return { cause: parseCause(parsed.cause, `${path}.cause`), as_of_trading_day: integer(parsed.as_of_trading_day, `${path}.as_of_trading_day`, 0) } }
function parseCause(value: unknown, path: string): BeliefCause { if (value === "HorizonExpired") return value; const [tag, payload] = tagged(value, path); if (tag === "NewMaterial") { exact(payload, ["report"], `${path}.NewMaterial`); return { NewMaterial: { report: publicationId(payload.report, `${path}.NewMaterial.report`) } } } if (tag === "Correction") { exact(payload, ["report"], `${path}.Correction`); return { Correction: { report: publicationId(payload.report, `${path}.Correction.report`) } } } if (tag === "CreditDefault") { exact(payload, ["announcement"], `${path}.CreditDefault`); return { CreditDefault: { announcement: publicationId(payload.announcement, `${path}.CreditDefault.announcement`) } } } if (tag === "ExperienceFailure") { exact(payload, ["order"], `${path}.ExperienceFailure`); return { ExperienceFailure: { order: orderId(payload.order, `${path}.ExperienceFailure.order`) } } } if (tag === "ProfitableExit") { exact(payload, ["order"], `${path}.ProfitableExit`); return { ProfitableExit: { order: orderId(payload.order, `${path}.ProfitableExit.order`) } } } throw new Error(`存档 ${path}.${tag} 不是已知信念成因`) }
