import { SaveSchemaError, array, exact, integer, nullable, oneOf, record } from "../primitives.ts"
import { externalTag, money, orderId, tagged } from "./common.ts"

import { parsePlanId, parsePlanU64 } from "../../../utils/plan-identity.ts"
import type { PlanId } from "../../../types/generated/PlanId.ts"
import type { PlanEntry } from "../../../types/generated/PlanEntry.ts"
import type { RepurchaseExecutionPlan } from "../../../types/generated/RepurchaseExecutionPlan.ts"
import type { AccountId } from "../../../types/generated/AccountId.ts"

export type OwnerSequence = { readonly owner: AccountId; readonly next_sequence: string | null }
export type PlanBook = { readonly policy: PlanPolicy; readonly owner_sequences: readonly OwnerSequence[]; readonly plans: readonly PlanEntry[] }
export type PlanPolicy = { readonly reverse_revision_threshold_bp: number; readonly review_signal_delta_bp: number; readonly review_price_change_bp: number }
export type TradingPlan = { readonly plan_id: PlanId; readonly account: AccountId; readonly code: number; readonly direction: Side; readonly target: PlanTarget; readonly filled_qty: number; readonly opinion: PlanOpinion; readonly confidence_bp: number; readonly urgency: Urgency; readonly status: PlanStatus; readonly version: number; readonly last_revision: RevisionRecord | null; readonly last_resume: ResumeReason | null; readonly created_trading_day: number; readonly horizon_trading_days: number; readonly review: ReviewConditions; readonly active_child_order_id: number | null; readonly execution?: PlanExecutionLeg; readonly last_event_trading_day: number }
/** 母单执行子状态：一次执行腿的调度参数与唯一在途子单事实（架构项 8：执行状态唯一所有者是计划本体）。 */
export type PlanExecutionLeg = { readonly child_qty: number; readonly limit_price: string; readonly expires_market_minute: number; readonly active_child_remaining_qty: number | null }
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
export type ReviewConditions = { readonly min_signal_delta_bp: number; readonly min_price_change_bp: number; readonly last_review_signal_score_bp: number; readonly last_review_trading_day: number; readonly last_review_price: string | null; readonly last_review_acquired_count: number; readonly last_review_resources: ReviewResources | null }
export type ReviewResources = { readonly cash: string; readonly frozen_cash: string; readonly held_qty: number; readonly t1_locked: number }

const SIDE = ["Buy", "Sell"] as const
const SOURCE = ["Fundamental", "Trend", "PriceVolume", "Technical", "Experience", "Blended"] as const
const URGENCY = ["Patient", "Normal", "Urgent"] as const
const PAUSE = ["IntradayDropAcceleration", "RiskPressure", "AdverseSelection"] as const
const TERMINATION = ["HorizonExpired", "FundsUnavailable", "Cancelled", "FilledBeyondTarget"] as const
const RESUME = ["TriggerCleared", "OpinionReaffirmed"] as const
const REVISION = ["SignalShift", "PriceMove", "ConstraintsChanged", "RiskTriggered", "HorizonReview", "NewInformation"] as const

export function parsePlanBook(value: unknown, path = "plans"): PlanBook {
  const parsed = record(value, path)
  exact(parsed, ["policy", "owner_sequences", "plans"], path)
  const policy = parsePlanPolicy(parsed.policy, `${path}.policy`)
  const owner_sequences = array(parsed.owner_sequences, `${path}.owner_sequences`).map((entry, index) => parseOwnerSequence(entry, `${path}.owner_sequences[${index}]`))
  const plans = array(parsed.plans, `${path}.plans`).map((entry, index) => parsePlanEntry(entry, `${path}.plans[${index}]`))
  validatePlanBookIdentities(owner_sequences, plans.map(summarizePlanEntry), path)
  return { policy, owner_sequences, plans }
}

export type PlanIdentitySummary = Readonly<{ index: number; plan_id: PlanId; stock: number; terminal: boolean }>

/** 跨行唯一性只需身份和状态摘要，不保留整张计划对象图。 */
export function summarizePlanEntry(entry: PlanEntry, index: number): PlanIdentitySummary {
  const plan = "Investor" in entry ? entry.Investor : entry.IssuerRepurchase
  const stock = "Investor" in entry ? entry.Investor.code : entry.IssuerRepurchase.stock
  const terminal = "Investor" in entry
    ? entry.Investor.status === "Completed" || (typeof entry.Investor.status === "object" && "Terminated" in entry.Investor.status)
    : entry.IssuerRepurchase.status === "Closed"
  return { index, plan_id: plan.plan_id, stock, terminal }
}

/** 完整图和分片入口共享 owner 游标及 PlanId/活动股票唯一性检查。 */
export function validatePlanBookIdentities(owner_sequences: readonly OwnerSequence[], plans: Iterable<PlanIdentitySummary>, path = "plans"): void {
  const cursors = new Map<string, string | null>()
  for (const [index, cursor] of owner_sequences.entries()) {
    if (cursors.has(cursor.owner)) throw new SaveSchemaError(`${path}.owner_sequences[${index}].owner`, "包含重复的 owner")
    cursors.set(cursor.owner, cursor.next_sequence)
  }
  const identities = new Map<string, Set<string>>()
  const activeStocks = new Map<string, Set<number>>()
  for (const summary of plans) {
    const { index, stock, terminal, plan_id } = summary
    const planPath = `${path}.plans[${index}]`
    const { owner, sequence } = plan_id
    const sequences = identities.get(owner) ?? new Set<string>()
    if (sequences.has(sequence)) throw new SaveSchemaError(`${planPath}.plan_id`, "包含重复的 PlanId")
    sequences.add(sequence)
    identities.set(owner, sequences)
    const next = cursors.get(owner)
    if (next === undefined) throw new SaveSchemaError(`${path}.owner_sequences`, `缺少计划 owner=${owner} 的游标`)
    if (next !== null && BigInt(next) <= BigInt(sequence)) throw new SaveSchemaError(`${path}.owner_sequences`, "next_sequence 必须大于该 owner 的全部已存 sequence")
    if (!terminal) {
      const stocks = activeStocks.get(owner) ?? new Set<number>()
      if (stocks.has(stock)) throw new SaveSchemaError(planPath, "同一账户和股票最多允许一个非终止计划")
      stocks.add(stock)
      activeStocks.set(owner, stocks)
    }
  }
}

/** 外部 tag 是必填来源事实；不接受旧裸结构或隐式 Investor。 */
export function parsePlanEntry(value: unknown, path: string): PlanEntry {
  const [tag, payload] = externalTag(value, path)
  if (tag === "Investor") return { Investor: parseTradingPlan(payload, `${path}.Investor`) }
  if (tag === "IssuerRepurchase") return { IssuerRepurchase: parseRepurchasePlan(payload, `${path}.IssuerRepurchase`) }
  throw new SaveSchemaError(path, `未知计划 origin：${tag}`)
}

function parseRepurchasePlan(value: unknown, path: string): RepurchaseExecutionPlan {
  const parsed = record(value, path)
  exact(parsed, ["plan_id", "issuer", "stock", "status", "execution"], path)
  const execution = nullable(parsed.execution, `${path}.execution`, parseExecutionLeg)
  if (execution !== null && (execution.child_qty > 4_294_967_295 || BigInt(execution.limit_price) <= 0n || (execution.active_child_remaining_qty !== null && (execution.active_child_remaining_qty <= 0 || execution.active_child_remaining_qty > 4_294_967_295 || execution.active_child_remaining_qty > execution.child_qty)))) {
    throw new SaveSchemaError(`${path}.execution`, "公司执行腿价格或活动余量非法")
  }
  const status = oneOf(parsed.status, `${path}.status`, ["Funded", "Draining", "Closed"] as const)
  if (status === "Closed" && execution !== null && execution.active_child_remaining_qty !== null) throw new SaveSchemaError(path, "Closed 公司计划不能保留活动子单余量")
  return { plan_id: parsePlanId(parsed.plan_id, `${path}.plan_id`), issuer: parseStock(parsed.issuer, `${path}.issuer`), stock: parseStock(parsed.stock, `${path}.stock`), status, execution }
}

export function parseTradingPlan(value: unknown, path: string): TradingPlan {
  const parsed = record(value, path)
  // execution 是可选键：缺省合法（尚未执行或腿已结束）；出现就必须是完整执行腿。
  const withExecution = "execution" in parsed ? [...TRADING_PLAN_KEYS, "execution"] : TRADING_PLAN_KEYS
  exact(parsed, withExecution, path)
  const plan_id = parsePlanId(parsed.plan_id, `${path}.plan_id`)
  const account = parsePlanU64(parsed.account, `${path}.account`)
  if (plan_id.owner !== account) throw new SaveSchemaError(`${path}.plan_id.owner`, "必须与 plan.account 一致")
  return { plan_id, account, code: parseStock(parsed.code, `${path}.code`), direction: oneOf(parsed.direction, `${path}.direction`, SIDE), target: parseTarget(parsed.target, `${path}.target`), filled_qty: integer(parsed.filled_qty, `${path}.filled_qty`, 0), opinion: parseOpinion(parsed.opinion, `${path}.opinion`), confidence_bp: integer(parsed.confidence_bp, `${path}.confidence_bp`, 0), urgency: oneOf(parsed.urgency, `${path}.urgency`, URGENCY), status: parseStatus(parsed.status, `${path}.status`), version: integer(parsed.version, `${path}.version`, 1), last_revision: nullable(parsed.last_revision, `${path}.last_revision`, parseRevision), last_resume: nullable(parsed.last_resume, `${path}.last_resume`, (nested, nestedPath) => oneOf(nested, nestedPath, RESUME)), created_trading_day: integer(parsed.created_trading_day, `${path}.created_trading_day`, 0), horizon_trading_days: integer(parsed.horizon_trading_days, `${path}.horizon_trading_days`, 1), review: parseReview(parsed.review, `${path}.review`), active_child_order_id: nullable(parsed.active_child_order_id, `${path}.active_child_order_id`, orderId), ...("execution" in parsed ? { execution: parseExecutionLeg(parsed.execution, `${path}.execution`) } : {}), last_event_trading_day: integer(parsed.last_event_trading_day, `${path}.last_event_trading_day`, 0) }
}

export function parseOwnerSequence(value: unknown, path: string): OwnerSequence {
  const parsed = record(value, path)
  exact(parsed, ["owner", "next_sequence"], path)
  return { owner: parsePlanU64(parsed.owner, `${path}.owner`), next_sequence: nullable(parsed.next_sequence, `${path}.next_sequence`, parsePlanU64) }
}

const TRADING_PLAN_KEYS = ["plan_id", "account", "code", "direction", "target", "filled_qty", "opinion", "confidence_bp", "urgency", "status", "version", "last_revision", "last_resume", "created_trading_day", "horizon_trading_days", "review", "active_child_order_id", "last_event_trading_day"] as const

/** 执行腿字段与 Rust `PlanExecutionLeg` wire 契约一致：分钟为数值 u64，价格为 Money 字符串。 */
function parseExecutionLeg(value: unknown, path: string): PlanExecutionLeg {
  const parsed = record(value, path)
  exact(parsed, ["child_qty", "limit_price", "expires_market_minute", "active_child_remaining_qty"], path)
  return { child_qty: integer(parsed.child_qty, `${path}.child_qty`, 1), limit_price: money(parsed.limit_price, `${path}.limit_price`), expires_market_minute: integer(parsed.expires_market_minute, `${path}.expires_market_minute`, 0), active_child_remaining_qty: nullable(parsed.active_child_remaining_qty, `${path}.active_child_remaining_qty`, (nested, nestedPath) => integer(nested, nestedPath, 0)) }
}

export function parsePlanPolicy(value: unknown, path: string): PlanPolicy { const parsed = record(value, path); exact(parsed, ["reverse_revision_threshold_bp", "review_signal_delta_bp", "review_price_change_bp"], path); return { reverse_revision_threshold_bp: integer(parsed.reverse_revision_threshold_bp, `${path}.reverse_revision_threshold_bp`), review_signal_delta_bp: integer(parsed.review_signal_delta_bp, `${path}.review_signal_delta_bp`), review_price_change_bp: integer(parsed.review_price_change_bp, `${path}.review_price_change_bp`) } }
/** 存档 wire 的股票身份：数值 StockId（ADR 0041）。 */
function parseStock(value: unknown, path: string): number { const parsed = integer(value, path, 0); if (parsed > 4_294_967_295) throw new Error(`存档 ${path} 超出 u32 范围`); return parsed }
function parseTarget(value: unknown, path: string): PlanTarget { const [tag, payload] = externalTag(value, path); if (tag === "ShareCount") return { ShareCount: integer(payload, `${path}.ShareCount`, 0) }; if (tag === "PositionFractionBp") return { PositionFractionBp: integer(payload, `${path}.PositionFractionBp`, 0) }; throw new Error(`存档 ${path}.${tag} 不是已知计划目标`) }
function parseOpinion(value: unknown, path: string): PlanOpinion { const parsed = record(value, path); exact(parsed, ["signal_score_bp", "source"], path); return { signal_score_bp: integer(parsed.signal_score_bp, `${path}.signal_score_bp`), source: oneOf(parsed.source, `${path}.source`, SOURCE) } }
function parseStatus(value: unknown, path: string): PlanStatus { if (value === "Active" || value === "Completed") return value; const [tag, payload] = tagged(value, path); if (tag === "Paused") { exact(payload, ["reason"], `${path}.Paused`); return { Paused: { reason: oneOf(payload.reason, `${path}.Paused.reason`, PAUSE) } } }; if (tag === "Terminated") { exact(payload, ["reason"], `${path}.Terminated`); return { Terminated: { reason: oneOf(payload.reason, `${path}.Terminated.reason`, TERMINATION) } } }; throw new Error(`存档 ${path}.${tag} 不是已知计划状态`) }
function parseRevision(value: unknown, path: string): RevisionRecord { const parsed = record(value, path); exact(parsed, ["version", "reason", "trading_day"], path); return { version: integer(parsed.version, `${path}.version`, 1), reason: oneOf(parsed.reason, `${path}.reason`, REVISION), trading_day: integer(parsed.trading_day, `${path}.trading_day`, 0) } }
function parseReview(value: unknown, path: string): ReviewConditions { const parsed = record(value, path); exact(parsed, ["min_signal_delta_bp", "min_price_change_bp", "last_review_signal_score_bp", "last_review_trading_day", "last_review_price", "last_review_acquired_count", "last_review_resources"], path); return { min_signal_delta_bp: integer(parsed.min_signal_delta_bp, `${path}.min_signal_delta_bp`), min_price_change_bp: integer(parsed.min_price_change_bp, `${path}.min_price_change_bp`), last_review_signal_score_bp: integer(parsed.last_review_signal_score_bp, `${path}.last_review_signal_score_bp`), last_review_trading_day: integer(parsed.last_review_trading_day, `${path}.last_review_trading_day`, 0), last_review_price: nullable(parsed.last_review_price, `${path}.last_review_price`, money), last_review_acquired_count: integer(parsed.last_review_acquired_count, `${path}.last_review_acquired_count`, 0), last_review_resources: nullable(parsed.last_review_resources, `${path}.last_review_resources`, parseReviewResources) } }
function parseReviewResources(value: unknown, path: string): ReviewResources {
  const parsed = record(value, path)
  exact(parsed, ["cash", "frozen_cash", "held_qty", "t1_locked"], path)
  const result = { cash: money(parsed.cash, `${path}.cash`), frozen_cash: money(parsed.frozen_cash, `${path}.frozen_cash`), held_qty: integer(parsed.held_qty, `${path}.held_qty`, 0), t1_locked: integer(parsed.t1_locked, `${path}.t1_locked`, 0) }
  if (BigInt(result.cash) < 0n || BigInt(result.frozen_cash) < 0n || BigInt(result.frozen_cash) > BigInt(result.cash) || result.t1_locked > result.held_qty) throw new Error(`存档 ${path} 的历史复核资源事实不一致`)
  return result
}
