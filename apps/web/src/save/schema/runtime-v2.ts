import { array, boolean, decimal, exact, integer, map, oneOf, record, safeIntegerKey, string } from "./primitives.ts"
import { u32 } from "./personal/common.ts"

const sides = ["Buy", "Sell"] as const
const retailStyles = ["Dormant", "LongTerm", "Noise", "DipBuyer", "Momentum", "Panic"] as const
const institutionStyles = ["DeepValue", "Growth", "Balanced", "Defensive", "ActiveTrader"] as const
const hotStyles = ["Momentum", "Reversal"] as const
const journals = ["PreSeal", "SealedBatch"] as const
const EXACT_FLOAT = /^[0-9a-f]{16}$/
const EXACT_FLOAT_EXPONENT_MASK = 0x7ff0000000000000n

export type FeeComponentsV2 = {
  readonly commission: number
  readonly stamp_tax: number
  readonly transfer_fee: number
}

export type EnvelopeKeyV2 = {
  readonly account: number
  readonly stock: string
  readonly order: number
  readonly side: (typeof sides)[number]
}

export type LiveEnvelopeV2 = {
  readonly key: EnvelopeKeyV2
  readonly live: { readonly cash: number; readonly shares: number }
  readonly audit: {
    readonly limit: number
    readonly remaining_qty: number
    readonly filled_qty: number
    readonly filled_value: number
    readonly nominal: FeeComponentsV2
    readonly charged: FeeComponentsV2
  }
}

type MomentumState = {
  readonly style: (typeof hotStyles)[number]
  readonly lookback: number
  readonly trend_threshold: string
  readonly order_size: number
  readonly volume_confirmation: string
  readonly max_stock_fraction: string
  readonly base_observation_probability: string
}

export type StrategyStateV2 =
  | { readonly ZiNoise: {
      readonly retail_style: (typeof retailStyles)[number]
      readonly arrival_rate: string
      readonly order_size_mean: number
      readonly chase_prob: string
      readonly tick_cents: number
      readonly dip_threshold: string
      readonly stop_loss_threshold: string
      readonly take_profit_threshold: string
      readonly volume_confirmation: string
      readonly max_stock_fraction: string
      readonly base_observation_probability: string
    } }
  | { readonly Momentum: MomentumState }
  | { readonly BeliefInstitution: {
      readonly style: (typeof institutionStyles)[number]
      readonly margin: string
      readonly order_size: number
      readonly max_stock_fraction: string
      readonly base_observation_probability: string
    } }

type ReceiptSourceV2 =
  | { readonly SealedIntent: string }
  | { readonly P0Expiry: number }
  | { readonly Auction: number }
  | { readonly DayEnd: number }

export type RetailReceiptIdentityV2 = {
  readonly index: string
  readonly local_key: {
    readonly journal: (typeof journals)[number]
    readonly source: ReceiptSourceV2
    readonly transition: { readonly envelope: EnvelopeKeyV2; readonly ordinal: string }
  }
}

export type SaveRuntimeV2 = {
  readonly poisoned: boolean
  readonly next_receipt_base: string
  readonly live_envelopes: readonly LiveEnvelopeV2[]
  readonly retail_projection_seen: readonly RetailReceiptIdentityV2[]
  readonly strategy_states: Readonly<Record<string, StrategyStateV2>>
}

function accountKey(value: string, path: string): void {
  safeIntegerKey(value, path)
}

function stock(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (parsed.length === 0) throw new Error(`存档 ${path} 股票代码不能为空`)
  return parsed
}

function money(value: unknown, path: string): number {
  return integer(value, path)
}

function boundedU32(value: unknown, path: string, minimum = 0): number {
  return u32(integer(value, path, minimum), path)
}

function exactFloat(value: unknown, path: string): string {
  const parsed = string(value, path)
  if (!EXACT_FLOAT.test(parsed)) throw new Error(`存档 ${path} 必须是 16 位小写十六进制浮点位串`)
  if ((BigInt(`0x${parsed}`) & EXACT_FLOAT_EXPONENT_MASK) === EXACT_FLOAT_EXPONENT_MASK) {
    throw new Error(`存档 ${path} 必须编码有限浮点数`)
  }
  return parsed
}

function feeComponents(value: unknown, path: string): FeeComponentsV2 {
  const parsed = record(value, path)
  exact(parsed, ["commission", "stamp_tax", "transfer_fee"], path)
  return {
    commission: money(parsed.commission, `${path}.commission`),
    stamp_tax: money(parsed.stamp_tax, `${path}.stamp_tax`),
    transfer_fee: money(parsed.transfer_fee, `${path}.transfer_fee`),
  }
}

function envelopeKey(value: unknown, path: string): EnvelopeKeyV2 {
  const parsed = record(value, path)
  exact(parsed, ["account", "stock", "order", "side"], path)
  return {
    account: integer(parsed.account, `${path}.account`, 0),
    stock: stock(parsed.stock, `${path}.stock`),
    order: integer(parsed.order, `${path}.order`, 1),
    side: oneOf(parsed.side, `${path}.side`, sides),
  }
}

function liveEnvelope(value: unknown, path: string): LiveEnvelopeV2 {
  const parsed = record(value, path)
  exact(parsed, ["key", "live", "audit"], path)
  const live = record(parsed.live, `${path}.live`)
  exact(live, ["cash", "shares"], `${path}.live`)
  const audit = record(parsed.audit, `${path}.audit`)
  exact(audit, ["limit", "remaining_qty", "filled_qty", "filled_value", "nominal", "charged"], `${path}.audit`)
  return {
    key: envelopeKey(parsed.key, `${path}.key`),
    live: {
      cash: money(live.cash, `${path}.live.cash`),
      shares: boundedU32(live.shares, `${path}.live.shares`),
    },
    audit: {
      limit: money(audit.limit, `${path}.audit.limit`),
      remaining_qty: boundedU32(audit.remaining_qty, `${path}.audit.remaining_qty`, 1),
      filled_qty: boundedU32(audit.filled_qty, `${path}.audit.filled_qty`),
      filled_value: money(audit.filled_value, `${path}.audit.filled_value`),
      nominal: feeComponents(audit.nominal, `${path}.audit.nominal`),
      charged: feeComponents(audit.charged, `${path}.audit.charged`),
    },
  }
}

function momentum(value: unknown, path: string): MomentumState {
  const parsed = record(value, path)
  exact(parsed, ["style", "lookback", "trend_threshold", "order_size", "volume_confirmation", "max_stock_fraction", "base_observation_probability"], path)
  return {
    style: oneOf(parsed.style, `${path}.style`, hotStyles),
    lookback: integer(parsed.lookback, `${path}.lookback`, 2),
    trend_threshold: exactFloat(parsed.trend_threshold, `${path}.trend_threshold`),
    order_size: boundedU32(parsed.order_size, `${path}.order_size`, 1),
    volume_confirmation: exactFloat(parsed.volume_confirmation, `${path}.volume_confirmation`),
    max_stock_fraction: exactFloat(parsed.max_stock_fraction, `${path}.max_stock_fraction`),
    base_observation_probability: exactFloat(parsed.base_observation_probability, `${path}.base_observation_probability`),
  }
}

function strategyState(value: unknown, path: string): StrategyStateV2 {
  const parsed = record(value, path)
  const variants = Object.keys(parsed)
  if (variants.length !== 1) throw new Error(`存档 ${path} 必须是单一 StrategyState 变体`)
  switch (variants[0]) {
    case "ZiNoise": {
      const state = record(parsed.ZiNoise, `${path}.ZiNoise`)
      exact(state, ["retail_style", "arrival_rate", "order_size_mean", "chase_prob", "tick_cents", "dip_threshold", "stop_loss_threshold", "take_profit_threshold", "volume_confirmation", "max_stock_fraction", "base_observation_probability"], `${path}.ZiNoise`)
      return { ZiNoise: {
        retail_style: oneOf(state.retail_style, `${path}.ZiNoise.retail_style`, retailStyles),
        arrival_rate: exactFloat(state.arrival_rate, `${path}.ZiNoise.arrival_rate`),
        order_size_mean: boundedU32(state.order_size_mean, `${path}.ZiNoise.order_size_mean`, 1),
        chase_prob: exactFloat(state.chase_prob, `${path}.ZiNoise.chase_prob`),
        tick_cents: integer(state.tick_cents, `${path}.ZiNoise.tick_cents`, 1),
        dip_threshold: exactFloat(state.dip_threshold, `${path}.ZiNoise.dip_threshold`),
        stop_loss_threshold: exactFloat(state.stop_loss_threshold, `${path}.ZiNoise.stop_loss_threshold`),
        take_profit_threshold: exactFloat(state.take_profit_threshold, `${path}.ZiNoise.take_profit_threshold`),
        volume_confirmation: exactFloat(state.volume_confirmation, `${path}.ZiNoise.volume_confirmation`),
        max_stock_fraction: exactFloat(state.max_stock_fraction, `${path}.ZiNoise.max_stock_fraction`),
        base_observation_probability: exactFloat(state.base_observation_probability, `${path}.ZiNoise.base_observation_probability`),
      } }
    }
    case "Momentum":
      return { Momentum: momentum(parsed.Momentum, `${path}.Momentum`) }
    case "BeliefInstitution": {
      const state = record(parsed.BeliefInstitution, `${path}.BeliefInstitution`)
      exact(state, ["style", "margin", "order_size", "max_stock_fraction", "base_observation_probability"], `${path}.BeliefInstitution`)
      return { BeliefInstitution: {
        style: oneOf(state.style, `${path}.BeliefInstitution.style`, institutionStyles),
        margin: exactFloat(state.margin, `${path}.BeliefInstitution.margin`),
        order_size: boundedU32(state.order_size, `${path}.BeliefInstitution.order_size`, 1),
        max_stock_fraction: exactFloat(state.max_stock_fraction, `${path}.BeliefInstitution.max_stock_fraction`),
        base_observation_probability: exactFloat(state.base_observation_probability, `${path}.BeliefInstitution.base_observation_probability`),
      } }
    }
    default:
      throw new Error(`存档 ${path} 包含无效 StrategyState 变体`)
  }
}

function receiptSource(value: unknown, path: string): ReceiptSourceV2 {
  const parsed = record(value, path)
  const variants = Object.keys(parsed)
  if (variants.length !== 1) throw new Error(`存档 ${path} 必须是单一收据来源变体`)
  switch (variants[0]) {
    case "SealedIntent": return { SealedIntent: decimal(parsed.SealedIntent, `${path}.SealedIntent`) }
    case "P0Expiry": return { P0Expiry: boundedU32(parsed.P0Expiry, `${path}.P0Expiry`) }
    case "Auction": return { Auction: boundedU32(parsed.Auction, `${path}.Auction`) }
    case "DayEnd": return { DayEnd: boundedU32(parsed.DayEnd, `${path}.DayEnd`) }
    default: throw new Error(`存档 ${path} 包含无效收据来源变体`)
  }
}

function receiptIdentity(value: unknown, path: string): RetailReceiptIdentityV2 {
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

export function parseSaveRuntimeV2(value: unknown, path = "runtime_v2"): SaveRuntimeV2 {
  const parsed = record(value, path)
  exact(parsed, ["poisoned", "next_receipt_base", "live_envelopes", "retail_projection_seen", "strategy_states"], path)
  return {
    poisoned: boolean(parsed.poisoned, `${path}.poisoned`),
    next_receipt_base: decimal(parsed.next_receipt_base, `${path}.next_receipt_base`),
    live_envelopes: array(parsed.live_envelopes, `${path}.live_envelopes`).map((item, index) => liveEnvelope(item, `${path}.live_envelopes[${index}]`)),
    retail_projection_seen: array(parsed.retail_projection_seen, `${path}.retail_projection_seen`).map((item, index) => receiptIdentity(item, `${path}.retail_projection_seen[${index}]`)),
    strategy_states: map(parsed.strategy_states, `${path}.strategy_states`, accountKey, strategyState),
  }
}
