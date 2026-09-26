import type { Intent } from "../../types/engine"
import type { NpcAttentionState } from "../../types/generated/NpcAttentionState"
import type { NpcOrderLifecycle } from "../../types/generated/NpcOrderLifecycle"
import type { Order } from "../../types/generated/Order"
import type { ParentOrderPlan } from "../../types/generated/ParentOrderPlan"
import type { PendingNpcBatch } from "../../types/generated/PendingNpcBatch"
import { array, decimal, exact, integer, map, oneOf, record, safeIntegerKey, SaveSchemaError, string } from "./primitives.ts"

const side = ["Buy", "Sell"] as const

function stockCode(value: string, path: string): void {
  if (value.length === 0) throw new Error(`存档 ${path} 股票代码不能为空`)
}

function accountKey(value: string, path: string): void {
  safeIntegerKey(value, path)
}

function money(value: unknown, path: string): number {
  return integer(value, path)
}

function intent(value: unknown, path: string): Intent {
  const parsed = record(value, path)
  const keys = Object.keys(parsed)
  if (keys.length !== 1) throw new Error(`存档 ${path} 必须是单一意图变体`)
  switch (keys[0]) {
    case "PlaceLimit": {
      const payload = record(parsed.PlaceLimit, `${path}.PlaceLimit`)
      exact(payload, ["code", "side", "price", "qty"], `${path}.PlaceLimit`)
      return { PlaceLimit: { code: string(payload.code, `${path}.PlaceLimit.code`), side: oneOf(payload.side, `${path}.PlaceLimit.side`, side), price: money(payload.price, `${path}.PlaceLimit.price`), qty: integer(payload.qty, `${path}.PlaceLimit.qty`, 1) } }
    }
    case "PlaceMarket": {
      const payload = record(parsed.PlaceMarket, `${path}.PlaceMarket`)
      exact(payload, ["code", "side", "qty"], `${path}.PlaceMarket`)
      return { PlaceMarket: { code: string(payload.code, `${path}.PlaceMarket.code`), side: oneOf(payload.side, `${path}.PlaceMarket.side`, side), qty: integer(payload.qty, `${path}.PlaceMarket.qty`, 1) } }
    }
    case "Cancel": {
      const payload = record(parsed.Cancel, `${path}.Cancel`)
      exact(payload, ["code", "id"], `${path}.Cancel`)
      return { Cancel: { code: string(payload.code, `${path}.Cancel.code`), id: integer(payload.id, `${path}.Cancel.id`, 1) } }
    }
    default:
      throw new Error(`存档 ${path} 包含无效意图变体`)
  }
}

function pendingNpcBatch(value: unknown): PendingNpcBatch | null {
  if (value === null) return null
  const batch = record(value, "pending_npc")
  exact(batch, ["observed_tick", "observed_accounts", "intents", "dependencies"], "pending_npc")
  const intents = array(batch.intents, "pending_npc.intents").map((item, index): [number, Intent] => {
    const path = `pending_npc.intents[${index}]`
    const tuple = array(item, path)
    if (tuple.length !== 2) throw new SaveSchemaError(path, "必须是二元组")
    return [integer(tuple[0], `${path}[0]`, 1), intent(tuple[1], `${path}[1]`)]
  })
  const seen = new Set<string>()
  const dependencies = array(batch.dependencies, "pending_npc.dependencies").map((item, index): [number, number] => {
    const path = `pending_npc.dependencies[${index}]`
    const tuple = array(item, path)
    if (tuple.length !== 2) throw new SaveSchemaError(path, "必须是二元组")
    const before = integer(tuple[0], `${path}[0]`, 0)
    const after = integer(tuple[1], `${path}[1]`, 0)
    if (before >= after || after >= intents.length) {
      throw new SaveSchemaError(path, "必须指向队列中先撤单、后下单的两个位置")
    }
    const [beforeAccount, beforeIntent] = intents[before]!
    const [afterAccount, afterIntent] = intents[after]!
    if (!("Cancel" in beforeIntent) || "Cancel" in afterIntent) {
      throw new SaveSchemaError(path, "必须由撤单指向限价单或市价单")
    }
    const replacement = "PlaceLimit" in afterIntent ? afterIntent.PlaceLimit : afterIntent.PlaceMarket
    if (beforeAccount !== afterAccount || beforeIntent.Cancel.code !== replacement.code) {
      throw new SaveSchemaError(path, "必须关联同一账户、同一股票的撤单和新单")
    }
    const key = `${before}:${after}`
    if (seen.has(key)) throw new SaveSchemaError(path, "不能重复")
    seen.add(key)
    return [before, after]
  })
  return {
    observed_tick: integer(batch.observed_tick, "pending_npc.observed_tick", 0),
    observed_accounts: array(batch.observed_accounts, "pending_npc.observed_accounts").map((item, index) => integer(item, `pending_npc.observed_accounts[${index}]`, 1)),
    intents,
    dependencies,
  }
}

export function parseOrderState(value: Record<string, unknown>): Pick<import("../../types/engine").SaveSlot, "auction_orders" | "resting_orders" | "filled_orders" | "price_history" | "market_minute_closes" | "rng_state" | "npc_attention" | "parent_orders" | "npc_order_lifecycles" | "pending_player" | "pending_npc" | "next_order_id"> {
  const auction_orders = map(value.auction_orders, "auction_orders", stockCode, (items, path) => array(items, path).map((item, index) => { const order = record(item, `${path}[${index}]`); exact(order, ["owner", "side", "limit", "qty", "order_id"], `${path}[${index}]`); return { owner: integer(order.owner, `${path}[${index}].owner`, 0), side: oneOf(order.side, `${path}[${index}].side`, side), limit: money(order.limit, `${path}[${index}].limit`), qty: integer(order.qty, `${path}[${index}].qty`, 1), order_id: integer(order.order_id, `${path}[${index}].order_id`, 0) } }))
  const resting_orders = map(value.resting_orders, "resting_orders", stockCode, (items, path) => array(items, path).map((item, index): Order => { const order = record(item, `${path}[${index}]`); exact(order, ["id", "side", "price", "qty", "original_qty", "filled_qty", "filled_value", "owner", "seq"], `${path}[${index}]`); return { id: integer(order.id, `${path}[${index}].id`, 1), side: oneOf(order.side, `${path}[${index}].side`, side), price: money(order.price, `${path}[${index}].price`), qty: integer(order.qty, `${path}[${index}].qty`, 0), original_qty: integer(order.original_qty, `${path}[${index}].original_qty`, 1), filled_qty: integer(order.filled_qty, `${path}[${index}].filled_qty`, 0), filled_value: money(order.filled_value, `${path}[${index}].filled_value`), owner: integer(order.owner, `${path}[${index}].owner`, 0), seq: integer(order.seq, `${path}[${index}].seq`, 0) } }))
  const filled_orders = map(value.filled_orders, "filled_orders", stockCode, (items, path) => array(items, path).map((item, index) => { const order = record(item, `${path}[${index}]`); exact(order, ["id", "owner"], `${path}[${index}]`); return { id: integer(order.id, `${path}[${index}].id`, 1), owner: integer(order.owner, `${path}[${index}].owner`, 0) } }))
  const price_history = map(value.price_history, "price_history", stockCode, (items, path) => array(items, path).map((item, index) => money(item, `${path}[${index}]`)))
  const market_minute_closes = map(value.market_minute_closes, "market_minute_closes", stockCode, (items, path) => array(items, path).map((item, index) => { const close = record(item, `${path}[${index}]`); exact(close, ["absolute_trading_minute", "close"], `${path}[${index}]`); return { absolute_trading_minute: integer(close.absolute_trading_minute, `${path}[${index}].absolute_trading_minute`, 0), close: money(close.close, `${path}[${index}].close`) } }))
  const npc_attention = map(value.npc_attention, "npc_attention", accountKey, (item, path): NpcAttentionState => { const attention = record(item, path); exact(attention, ["base_probability", "next_attention_candidate_tick", "rng_state"], path); const probability = typeof attention.base_probability === "number" ? attention.base_probability : NaN; if (!Number.isFinite(probability) || probability <= 0 || probability > 1) throw new Error(`存档 ${path}.base_probability 无效`); return { base_probability: probability, next_attention_candidate_tick: decimal(attention.next_attention_candidate_tick, `${path}.next_attention_candidate_tick`), rng_state: decimal(attention.rng_state, `${path}.rng_state`) } })
  const parent_orders = map(value.parent_orders, "parent_orders", accountKey, (items, path) => map(items, path, stockCode, (item, itemPath): ParentOrderPlan => { const plan = record(item, itemPath); const keys = "linked_plan_id" in plan ? ["code", "side", "target_qty", "filled_qty", "child_qty", "active_child_order_id", "active_child_remaining_qty", "linked_plan_id", "limit_price", "expires_market_minute"] : ["code", "side", "target_qty", "filled_qty", "child_qty", "active_child_order_id", "active_child_remaining_qty", "limit_price", "expires_market_minute"]; exact(plan, keys, itemPath); return { code: string(plan.code, `${itemPath}.code`), side: oneOf(plan.side, `${itemPath}.side`, side), target_qty: integer(plan.target_qty, `${itemPath}.target_qty`, 1), filled_qty: integer(plan.filled_qty, `${itemPath}.filled_qty`, 0), child_qty: integer(plan.child_qty, `${itemPath}.child_qty`, 1), active_child_order_id: plan.active_child_order_id === null ? null : integer(plan.active_child_order_id, `${itemPath}.active_child_order_id`, 1), active_child_remaining_qty: plan.active_child_remaining_qty === null ? null : integer(plan.active_child_remaining_qty, `${itemPath}.active_child_remaining_qty`, 1), ...( "linked_plan_id" in plan ? { linked_plan_id: integer(plan.linked_plan_id, `${itemPath}.linked_plan_id`, 1) } : {}), limit_price: money(plan.limit_price, `${itemPath}.limit_price`), expires_market_minute: decimal(plan.expires_market_minute, `${itemPath}.expires_market_minute`) } }))
  const npc_order_lifecycles = array(value.npc_order_lifecycles, "npc_order_lifecycles").map((item, index): NpcOrderLifecycle => { const lifecycle = record(item, `npc_order_lifecycles[${index}]`); exact(lifecycle, ["account", "code", "order_id", "placed_market_minute", "expires_market_minute"], `npc_order_lifecycles[${index}]`); return { account: integer(lifecycle.account, `npc_order_lifecycles[${index}].account`, 0), code: string(lifecycle.code, `npc_order_lifecycles[${index}].code`), order_id: integer(lifecycle.order_id, `npc_order_lifecycles[${index}].order_id`, 1), placed_market_minute: integer(lifecycle.placed_market_minute, `npc_order_lifecycles[${index}].placed_market_minute`, 0), expires_market_minute: integer(lifecycle.expires_market_minute, `npc_order_lifecycles[${index}].expires_market_minute`, 0) } })
  const pending_player = array(value.pending_player, "pending_player").map((item, index) => { const tuple = array(item, `pending_player[${index}]`); if (tuple.length !== 2) throw new Error(`存档 pending_player[${index}] 必须是二元组`); return [integer(tuple[0], `pending_player[${index}][0]`, 0), intent(tuple[1], `pending_player[${index}][1]`)] as [number, Intent] })
  const pending_npc = pendingNpcBatch(value.pending_npc)
  return { auction_orders, resting_orders, filled_orders, price_history, market_minute_closes, rng_state: decimal(value.rng_state, "rng_state"), npc_attention, parent_orders, npc_order_lifecycles, pending_player, pending_npc, next_order_id: integer(value.next_order_id, "next_order_id", 1) }
}
