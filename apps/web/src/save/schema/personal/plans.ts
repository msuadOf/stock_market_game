import { array, exact, integer, map, nullable, oneOf, record } from "../primitives.ts"
import { externalTag, orderId, planKey, stockKey, tagged, type StringMap } from "./common.ts"

export type PlanBook = { readonly policy: PlanPolicy; readonly next_plan_seq: number; readonly plans: StringMap<TradingPlan> }
export type PlanPolicy = { readonly reverse_revision_threshold_bp: number; readonly review_signal_delta_bp: number; readonly review_price_change_bp: number }
export type TradingPlan = { readonly plan_id: number; readonly account: number; readonly code: string; readonly direction: Side; readonly target: PlanTarget; readonly filled_qty: number; readonly opinion: PlanOpinion; readonly confidence_bp: number; readonly urgency: Urgency; readonly status: PlanStatus; readonly version: number; readonly last_revision: RevisionRecord | null; readonly last_resume: ResumeReason | null; readonly created_trading_day: number; readonly horizon_trading_days: number; readonly review: ReviewConditions; readonly active_child_order_id: number | null; readonly last_event_trading_day: number }
export type Side = "Buy" | "Sell"
export type PlanTarget = { readonly ShareCount: number } | { readonly PositionFractionBp: number }
export type PlanOpinion = { readonly signal_score_bp: number; readonly source: OpinionSource }
export type OpinionSource = "Fundamental" | "Trend" | "PriceVolume" | "Technical" | "Experience" | "Blended"
export type Urgency = "Patient" | "Normal" | "Urgent"
export type PlanStatus = "Active" | { readonly Paused: { readonly reason: PauseReason } } | "Completed" | { readonly Terminated: { readonly reason: TerminationReason } }
export type PauseReason = "IntradayDropAcceleration" | "RiskPressure" | "AdverseSelection"
export type TerminationReason = "HorizonExpired" | "FundsUnavailable" | "Cancelled" | "FilledBeyondTarget"
export type ResumeReason = "TriggerCleared" | "OpinionReaffirmed"
export type RevisionRecord = { readonly version: number; readonly reason: RevisionReason; readonly trading_day: number }
export type RevisionReason = "SignalShift" | "PriceMove" | "ConstraintsChanged" | "RiskTriggered" | "HorizonReview" | "NewInformation"
export type ReviewConditions = { readonly min_signal_delta_bp: number; readonly min_price_change_bp: number; readonly last_review_signal_score_bp: number; readonly last_review_trading_day: number }
export type PendingPlanEvent = { readonly Accepted: { readonly plan_id: number; readonly order_id: number; readonly trading_day: number } } | { readonly Filled: { readonly plan_id: number; readonly order_id: number; readonly qty: number; readonly trading_day: number } } | { readonly DayEnded: { readonly plan_id: number; readonly trading_day: number } }

const SIDE = ["Buy", "Sell"] as const
const SOURCE = ["Fundamental", "Trend", "PriceVolume", "Technical", "Experience", "Blended"] as const
const URGENCY = ["Patient", "Normal", "Urgent"] as const
const PAUSE = ["IntradayDropAcceleration", "RiskPressure", "AdverseSelection"] as const
const TERMINATION = ["HorizonExpired", "FundsUnavailable", "Cancelled", "FilledBeyondTarget"] as const
const RESUME = ["TriggerCleared", "OpinionReaffirmed"] as const
const REVISION = ["SignalShift", "PriceMove", "ConstraintsChanged", "RiskTriggered", "HorizonReview", "NewInformation"] as const

export function parsePlanBook(value: unknown, path = "plans"): PlanBook {
  const parsed = record(value, path)
  exact(parsed, ["policy", "next_plan_seq", "plans"], path)
  return { policy: parsePolicy(parsed.policy, `${path}.policy`), next_plan_seq: integer(parsed.next_plan_seq, `${path}.next_plan_seq`, 0), plans: map(parsed.plans, `${path}.plans`, planKey, parseTradingPlan) }
}

export function parseTradingPlan(value: unknown, path: string): TradingPlan {
  const parsed = record(value, path)
  exact(parsed, ["plan_id", "account", "code", "direction", "target", "filled_qty", "opinion", "confidence_bp", "urgency", "status", "version", "last_revision", "last_resume", "created_trading_day", "horizon_trading_days", "review", "active_child_order_id", "last_event_trading_day"], path)
  return { plan_id: integer(parsed.plan_id, `${path}.plan_id`, 0), account: integer(parsed.account, `${path}.account`, 0), code: parseStock(parsed.code, `${path}.code`), direction: oneOf(parsed.direction, `${path}.direction`, SIDE), target: parseTarget(parsed.target, `${path}.target`), filled_qty: integer(parsed.filled_qty, `${path}.filled_qty`, 0), opinion: parseOpinion(parsed.opinion, `${path}.opinion`), confidence_bp: integer(parsed.confidence_bp, `${path}.confidence_bp`, 0), urgency: oneOf(parsed.urgency, `${path}.urgency`, URGENCY), status: parseStatus(parsed.status, `${path}.status`), version: integer(parsed.version, `${path}.version`, 1), last_revision: nullable(parsed.last_revision, `${path}.last_revision`, parseRevision), last_resume: nullable(parsed.last_resume, `${path}.last_resume`, (nested, nestedPath) => oneOf(nested, nestedPath, RESUME)), created_trading_day: integer(parsed.created_trading_day, `${path}.created_trading_day`, 0), horizon_trading_days: integer(parsed.horizon_trading_days, `${path}.horizon_trading_days`, 1), review: parseReview(parsed.review, `${path}.review`), active_child_order_id: nullable(parsed.active_child_order_id, `${path}.active_child_order_id`, orderId), last_event_trading_day: integer(parsed.last_event_trading_day, `${path}.last_event_trading_day`, 0) }
}

export function parsePendingPlanEvents(value: unknown, path = "pending_plan_events"): readonly PendingPlanEvent[] {
  return array(value, path).map((event, index) => parsePendingPlanEvent(event, `${path}[${index}]`))
}

function parsePolicy(value: unknown, path: string): PlanPolicy { const parsed = record(value, path); exact(parsed, ["reverse_revision_threshold_bp", "review_signal_delta_bp", "review_price_change_bp"], path); return { reverse_revision_threshold_bp: integer(parsed.reverse_revision_threshold_bp, `${path}.reverse_revision_threshold_bp`), review_signal_delta_bp: integer(parsed.review_signal_delta_bp, `${path}.review_signal_delta_bp`), review_price_change_bp: integer(parsed.review_price_change_bp, `${path}.review_price_change_bp`) } }
function parseStock(value: unknown, path: string): string { if (typeof value !== "string") throw new Error(`存档 ${path} 必须是字符串`); stockKey(value, path); return value }
function parseTarget(value: unknown, path: string): PlanTarget { const [tag, payload] = externalTag(value, path); if (tag === "ShareCount") return { ShareCount: integer(payload, `${path}.ShareCount`, 0) }; if (tag === "PositionFractionBp") return { PositionFractionBp: integer(payload, `${path}.PositionFractionBp`, 0) }; throw new Error(`存档 ${path}.${tag} 不是已知计划目标`) }
function parseOpinion(value: unknown, path: string): PlanOpinion { const parsed = record(value, path); exact(parsed, ["signal_score_bp", "source"], path); return { signal_score_bp: integer(parsed.signal_score_bp, `${path}.signal_score_bp`), source: oneOf(parsed.source, `${path}.source`, SOURCE) } }
function parseStatus(value: unknown, path: string): PlanStatus { if (value === "Active" || value === "Completed") return value; const [tag, payload] = tagged(value, path); if (tag === "Paused") { exact(payload, ["reason"], `${path}.Paused`); return { Paused: { reason: oneOf(payload.reason, `${path}.Paused.reason`, PAUSE) } } }; if (tag === "Terminated") { exact(payload, ["reason"], `${path}.Terminated`); return { Terminated: { reason: oneOf(payload.reason, `${path}.Terminated.reason`, TERMINATION) } } }; throw new Error(`存档 ${path}.${tag} 不是已知计划状态`) }
function parseRevision(value: unknown, path: string): RevisionRecord { const parsed = record(value, path); exact(parsed, ["version", "reason", "trading_day"], path); return { version: integer(parsed.version, `${path}.version`, 1), reason: oneOf(parsed.reason, `${path}.reason`, REVISION), trading_day: integer(parsed.trading_day, `${path}.trading_day`, 0) } }
function parseReview(value: unknown, path: string): ReviewConditions { const parsed = record(value, path); exact(parsed, ["min_signal_delta_bp", "min_price_change_bp", "last_review_signal_score_bp", "last_review_trading_day"], path); return { min_signal_delta_bp: integer(parsed.min_signal_delta_bp, `${path}.min_signal_delta_bp`), min_price_change_bp: integer(parsed.min_price_change_bp, `${path}.min_price_change_bp`), last_review_signal_score_bp: integer(parsed.last_review_signal_score_bp, `${path}.last_review_signal_score_bp`), last_review_trading_day: integer(parsed.last_review_trading_day, `${path}.last_review_trading_day`, 0) } }
function parsePendingPlanEvent(value: unknown, path: string): PendingPlanEvent { const [tag, payload] = tagged(value, path); if (tag === "Accepted") { exact(payload, ["plan_id", "order_id", "trading_day"], `${path}.Accepted`); return { Accepted: { plan_id: integer(payload.plan_id, `${path}.Accepted.plan_id`, 0), order_id: orderId(payload.order_id, `${path}.Accepted.order_id`), trading_day: integer(payload.trading_day, `${path}.Accepted.trading_day`, 0) } } }; if (tag === "Filled") { exact(payload, ["plan_id", "order_id", "qty", "trading_day"], `${path}.Filled`); return { Filled: { plan_id: integer(payload.plan_id, `${path}.Filled.plan_id`, 0), order_id: orderId(payload.order_id, `${path}.Filled.order_id`), qty: integer(payload.qty, `${path}.Filled.qty`, 0), trading_day: integer(payload.trading_day, `${path}.Filled.trading_day`, 0) } } }; if (tag === "DayEnded") { exact(payload, ["plan_id", "trading_day"], `${path}.DayEnded`); return { DayEnded: { plan_id: integer(payload.plan_id, `${path}.DayEnded.plan_id`, 0), trading_day: integer(payload.trading_day, `${path}.DayEnded.trading_day`, 0) } } }; throw new Error(`存档 ${path}.${tag} 不是已知待应用计划事件`) }
