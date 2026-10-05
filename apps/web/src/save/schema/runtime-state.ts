import { money as parseCanonicalMoney } from "./primitives.ts"
import { accountId, SaveSchemaError, array, boolean, civilDate, decimal, exact, integer, map, oneOf, record, string } from "./primitives.ts"
import { u32 } from "./personal/common.ts"
import { parseActiveMinuteHistory } from "./retained-history.ts"

const sides = ["Buy", "Sell"] as const
const retailStyles = ["Dormant", "LongTerm", "Noise", "DipBuyer", "Momentum", "Panic"] as const
const institutionStyles = ["DeepValue", "Growth", "Balanced", "Defensive", "ActiveTrader"] as const
const hotStyles = ["Momentum", "Reversal"] as const
const journals = ["PreSeal", "SealedBatch"] as const
const EXACT_FLOAT = /^[0-9a-f]{16}$/
const EXACT_FLOAT_EXPONENT_MASK = 0x7ff0000000000000n

export type SavedFeeComponents = {
  readonly commission: string
  readonly stamp_tax: string
  readonly transfer_fee: string
}

export type SavedEnvelopeKey = {
  readonly account: string
  readonly stock: string
  readonly order: number
  readonly side: (typeof sides)[number]
}

export type SavedLiveEnvelope = {
  readonly key: SavedEnvelopeKey
  readonly charged: SavedFeeComponents
}

type MomentumState = {
  readonly style: (typeof hotStyles)[number]
  readonly lookback: number
  readonly trend_threshold: string
  readonly order_size: number
  readonly volume_confirmation: string
  readonly base_observation_probability: string
}

export type RuntimeStrategyState =
  | { readonly ZiNoise: {
      readonly retail_style: (typeof retailStyles)[number]
      readonly arrival_rate: string
      readonly order_size_mean: number
      readonly chase_prob: string
      readonly dip_threshold: string
      readonly stop_loss_threshold: string
      readonly take_profit_threshold: string
      readonly volume_confirmation: string
      readonly position_step_bp: number
      readonly base_observation_probability: string
    } }
  | { readonly Momentum: MomentumState }
  | { readonly BeliefInstitution: {
      readonly style: (typeof institutionStyles)[number]
      readonly margin: string
      readonly order_size: number
      readonly position_step_bp: number
      readonly base_observation_probability: string
    } }

type SavedReceiptSource =
  | { readonly SealedIntent: string }
  | { readonly QuoteExpiry: number }
  | { readonly Auction: number }
  | { readonly DayEnd: number }

export type SavedRetailReceiptIdentity = {
  readonly index: string
  readonly local_key: {
    readonly journal: (typeof journals)[number]
    readonly source: SavedReceiptSource
    readonly transition: { readonly envelope: SavedEnvelopeKey; readonly ordinal: string }
  }
}

export type SavedRuntimeState = {
  readonly active_minute_history: ReturnType<typeof parseActiveMinuteHistory>
  readonly poisoned: boolean
  readonly next_receipt_base: string
  readonly live_envelopes: readonly SavedLiveEnvelope[]
  readonly retail_projection_seen: readonly SavedRetailReceiptIdentity[]
  readonly strategy_states: Readonly<Record<string, RuntimeStrategyState>>
  readonly personal_trade_confirmations: Readonly<Record<string, readonly SavedPersonalTradeConfirmation[]>>
}

export type SavedPersonalTradeConfirmation = {
  readonly receipt_id: string
  readonly civil_date: string
  readonly code: string
  readonly side: (typeof sides)[number]
  readonly price: string
  readonly quantity_shares: number
  readonly gross: string
  readonly actual_fees: SavedFeeComponents
}

function personalConfirmation(value: unknown, path: string): SavedPersonalTradeConfirmation {
  const parsed = record(value, path)
  exact(parsed, ["receipt_id", "civil_date", "code", "side", "price", "quantity_shares", "gross", "actual_fees"], path)
  const receipt = decimal(parsed.receipt_id, `${path}.receipt_id`)
  if (!/^(0|[1-9]\d*)$/.test(receipt)) throw new SaveSchemaError(`${path}.receipt_id`, "必须为规范u64十进制字符串")
  const price = money(parsed.price, `${path}.price`)
  const quantity = boundedU32(parsed.quantity_shares, `${path}.quantity_shares`, 1)
  const gross = money(parsed.gross, `${path}.gross`)
  const fees = feeComponents(parsed.actual_fees, `${path}.actual_fees`)
  if (BigInt(price) <= 0n || BigInt(price) * BigInt(quantity) !== BigInt(gross)) throw new SaveSchemaError(path, "成交价乘股数必须等于正成交额")
  if (Object.values(fees).some((fee) => BigInt(fee) < 0n)) throw new SaveSchemaError(`${path}.actual_fees`, "实际费用不能为负数")
  return { receipt_id: receipt, civil_date: civilDate(parsed.civil_date, `${path}.civil_date`), code: stock(parsed.code, `${path}.code`), side: oneOf(parsed.side, `${path}.side`, sides), price, quantity_shares: quantity, gross, actual_fees: fees }
}

function accountKey(value: string, path: string): void {
  accountId(value, path)
}

function stock(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (parsed.length === 0) throw new Error(`存档 ${path} 股票代码不能为空`)
  return parsed
}

function money(value: unknown, path: string): string {
  return parseCanonicalMoney(value, path)
}

function boundedU32(value: unknown, path: string, minimum = 0): number {
  return u32(integer(value, path, minimum), path)
}

function positionStep(value: unknown, path: string): number {
  const parsed = integer(value, path, 0)
  if (parsed > 10_000) throw new SaveSchemaError(path, "单次仓位调整步幅不得超过 10000 基点")
  return parsed
}

function exactFloat(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (!EXACT_FLOAT.test(parsed)) throw new Error(`存档 ${path} 必须是 16 位小写十六进制浮点位串`)
  if ((BigInt(`0x${parsed}`) & EXACT_FLOAT_EXPONENT_MASK) === EXACT_FLOAT_EXPONENT_MASK) {
    throw new Error(`存档 ${path} 必须编码有限浮点数`)
  }
  return parsed
}

function feeComponents(value: unknown, path: string): SavedFeeComponents {
  const parsed = record(value, path)
  exact(parsed, ["commission", "stamp_tax", "transfer_fee"], path)
  return {
    commission: money(parsed.commission, `${path}.commission`),
    stamp_tax: money(parsed.stamp_tax, `${path}.stamp_tax`),
    transfer_fee: money(parsed.transfer_fee, `${path}.transfer_fee`),
  }
}

function envelopeKey(value: unknown, path: string): SavedEnvelopeKey {
  const parsed = record(value, path)
  exact(parsed, ["account", "stock", "order", "side"], path)
  return {
    account: accountId(parsed.account, `${path}.account`),
    stock: stock(parsed.stock, `${path}.stock`),
    order: integer(parsed.order, `${path}.order`, 1),
    side: oneOf(parsed.side, `${path}.side`, sides),
  }
}

function liveEnvelope(value: unknown, path: string): SavedLiveEnvelope {
  const parsed = record(value, path)
  exact(parsed, ["key", "charged"], path)
  return {
    key: envelopeKey(parsed.key, `${path}.key`),
    charged: feeComponents(parsed.charged, `${path}.charged`),
  }
}

function momentum(value: unknown, path: string): MomentumState {
  const parsed = record(value, path)
  exact(parsed, ["style", "lookback", "trend_threshold", "order_size", "volume_confirmation", "base_observation_probability"], path)
  return {
    style: oneOf(parsed.style, `${path}.style`, hotStyles),
    lookback: integer(parsed.lookback, `${path}.lookback`, 2),
    trend_threshold: exactFloat(parsed.trend_threshold, `${path}.trend_threshold`),
    order_size: boundedU32(parsed.order_size, `${path}.order_size`, 1),
    volume_confirmation: exactFloat(parsed.volume_confirmation, `${path}.volume_confirmation`),
    base_observation_probability: exactFloat(parsed.base_observation_probability, `${path}.base_observation_probability`),
  }
}

function strategyState(value: unknown, path: string): RuntimeStrategyState {
  const parsed = record(value, path)
  const variants = Object.keys(parsed)
  if (variants.length !== 1) throw new Error(`存档 ${path} 必须是单一 StrategyState 变体`)
  switch (variants[0]) {
    case "ZiNoise": {
      const state = record(parsed.ZiNoise, `${path}.ZiNoise`)
      exact(state, ["retail_style", "arrival_rate", "order_size_mean", "chase_prob", "dip_threshold", "stop_loss_threshold", "take_profit_threshold", "volume_confirmation", "position_step_bp", "base_observation_probability"], `${path}.ZiNoise`)
      return { ZiNoise: {
        retail_style: oneOf(state.retail_style, `${path}.ZiNoise.retail_style`, retailStyles),
        arrival_rate: exactFloat(state.arrival_rate, `${path}.ZiNoise.arrival_rate`),
        order_size_mean: boundedU32(state.order_size_mean, `${path}.ZiNoise.order_size_mean`, 1),
        chase_prob: exactFloat(state.chase_prob, `${path}.ZiNoise.chase_prob`),
        dip_threshold: exactFloat(state.dip_threshold, `${path}.ZiNoise.dip_threshold`),
        stop_loss_threshold: exactFloat(state.stop_loss_threshold, `${path}.ZiNoise.stop_loss_threshold`),
        take_profit_threshold: exactFloat(state.take_profit_threshold, `${path}.ZiNoise.take_profit_threshold`),
        volume_confirmation: exactFloat(state.volume_confirmation, `${path}.ZiNoise.volume_confirmation`),
        position_step_bp: positionStep(state.position_step_bp, `${path}.ZiNoise.position_step_bp`),
        base_observation_probability: exactFloat(state.base_observation_probability, `${path}.ZiNoise.base_observation_probability`),
      } }
    }
    case "Momentum":
      return { Momentum: momentum(parsed.Momentum, `${path}.Momentum`) }
    case "BeliefInstitution": {
      const state = record(parsed.BeliefInstitution, `${path}.BeliefInstitution`)
      exact(state, ["style", "margin", "order_size", "position_step_bp", "base_observation_probability"], `${path}.BeliefInstitution`)
      return { BeliefInstitution: {
        style: oneOf(state.style, `${path}.BeliefInstitution.style`, institutionStyles),
        margin: exactFloat(state.margin, `${path}.BeliefInstitution.margin`),
        order_size: boundedU32(state.order_size, `${path}.BeliefInstitution.order_size`, 1),
        position_step_bp: positionStep(state.position_step_bp, `${path}.BeliefInstitution.position_step_bp`),
        base_observation_probability: exactFloat(state.base_observation_probability, `${path}.BeliefInstitution.base_observation_probability`),
      } }
    }
    default:
      throw new Error(`存档 ${path} 包含无效 StrategyState 变体`)
  }
}

function receiptSource(value: unknown, path: string): SavedReceiptSource {
  const parsed = record(value, path)
  const variants = Object.keys(parsed)
  if (variants.length !== 1) throw new Error(`存档 ${path} 必须是单一收据来源变体`)
  switch (variants[0]) {
    case "SealedIntent": return { SealedIntent: decimal(parsed.SealedIntent, `${path}.SealedIntent`) }
    case "QuoteExpiry": return { QuoteExpiry: boundedU32(parsed.QuoteExpiry, `${path}.QuoteExpiry`) }
    case "Auction": return { Auction: boundedU32(parsed.Auction, `${path}.Auction`) }
    case "DayEnd": return { DayEnd: boundedU32(parsed.DayEnd, `${path}.DayEnd`) }
    default: throw new Error(`存档 ${path} 包含无效收据来源变体`)
  }
}

function receiptIdentity(value: unknown, path: string): SavedRetailReceiptIdentity {
  const parsed = record(value, path)
  exact(parsed, ["index", "local_key"], path)
  const localKey = record(parsed.local_key, `${path}.local_key`)
  exact(localKey, ["journal", "source", "transition"], `${path}.local_key`)
  const transition = record(localKey.transition, `${path}.local_key.transition`)
  exact(transition, ["envelope", "ordinal"], `${path}.local_key.transition`)
  return {
    index: decimal(parsed.index, `${path}.index`),
    local_key: {
      journal: oneOf(localKey.journal, `${path}.local_key.journal`, journals),
      source: receiptSource(localKey.source, `${path}.local_key.source`),
      transition: {
        envelope: envelopeKey(transition.envelope, `${path}.local_key.transition.envelope`),
        ordinal: decimal(transition.ordinal, `${path}.local_key.transition.ordinal`),
      },
    },
  }
}

export function parseSaveRuntime(value: unknown, path = "runtime_state"): SavedRuntimeState {
  const parsed = record(value, path)
  exact(parsed, ["active_minute_history", "poisoned", "next_receipt_base", "live_envelopes", "retail_projection_seen", "strategy_states", "personal_trade_confirmations"], path)
  const nextReceipt = decimal(parsed.next_receipt_base, `${path}.next_receipt_base`)
  const seen = new Set<string>()
  const confirmations = map(parsed.personal_trade_confirmations, `${path}.personal_trade_confirmations`, accountKey, (value, accountPath) => {
    let previous: bigint | null = null
    return array(value, accountPath).map((item, index) => {
      const itemPath = `${accountPath}[${index}]`
      const confirmation = personalConfirmation(item, itemPath)
      const receipt = BigInt(confirmation.receipt_id)
      if (receipt >= BigInt(nextReceipt) || (previous !== null && receipt <= previous) || seen.has(confirmation.receipt_id)) throw new SaveSchemaError(`${itemPath}.receipt_id`, "receipt必须唯一、递增且小于next_receipt_base")
      previous = receipt
      seen.add(confirmation.receipt_id)
      return confirmation
    })
  })
  return {
    active_minute_history: parseActiveMinuteHistory(parsed.active_minute_history, `${path}.active_minute_history`),
    poisoned: boolean(parsed.poisoned, `${path}.poisoned`),
    next_receipt_base: nextReceipt,
    personal_trade_confirmations: confirmations,
    live_envelopes: array(parsed.live_envelopes, `${path}.live_envelopes`).map((item, index) => liveEnvelope(item, `${path}.live_envelopes[${index}]`)),
    retail_projection_seen: array(parsed.retail_projection_seen, `${path}.retail_projection_seen`).map((item, index) => receiptIdentity(item, `${path}.retail_projection_seen[${index}]`)),
    strategy_states: map(parsed.strategy_states, `${path}.strategy_states`, accountKey, strategyState),
  }
}
