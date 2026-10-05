import type { StrictSaveEnvelope } from "../save/schema/root.ts"
import type { Event } from "../types/generated/Event.ts"
import type { PlayerWorkingOrder } from "../host/player-working-orders.ts"
export type { PlayerWorkingOrder } from "../host/player-working-orders.ts"

type WorkingOrderSource = Pick<StrictSaveEnvelope, "auction_orders" | "resting_orders">

export function projectPlayerOrders(slot: WorkingOrderSource): readonly PlayerWorkingOrder[] {
  const orders: PlayerWorkingOrder[] = []
  for (const [code, auctionOrders] of Object.entries(slot.auction_orders)) {
    for (const order of auctionOrders) {
      if (order.owner !== "0") continue
      orders.push({
        owner: "0",
        id: order.order_id,
        code,
        side: order.side,
        price: order.limit,
        remainingQty: order.qty,
        venue: "auction",
        frozen: order.side === "Buy" ? "cash" : "shares",
      })
    }
  }
  for (const [code, restingOrders] of Object.entries(slot.resting_orders)) {
    for (const order of restingOrders) {
      if (order.owner !== "0" || order.qty === 0) continue
      orders.push({
        owner: "0",
        id: order.id,
        code,
        side: order.side,
        price: order.price,
        remainingQty: order.qty,
        venue: "continuous",
        frozen: order.side === "Buy" ? "cash" : "shares",
      })
    }
  }
  return orders.sort((left, right) => left.id - right.id)
}

type PlayerOrderFact = { readonly event: Event }

export class PlayerOrderRefreshGate {
  private generation = 0

  next(): number {
    return ++this.generation
  }

  invalidate(): void {
    this.generation += 1
  }

  isCurrent(generation: number): boolean {
    return generation === this.generation
  }
}

export function playerOrderFactsRequireRefresh(facts: readonly PlayerOrderFact[], accountId: string | null = "0"): boolean {
  return facts.some(({ event }) =>
    ("OrderAccepted" in event && String(event.OrderAccepted.account) === accountId)
    || ("OrderCanceled" in event && String(event.OrderCanceled.account) === accountId)
    || ("Trade" in event && (String(event.Trade.maker) === accountId || String(event.Trade.taker) === accountId))
    || "AuctionCompleted" in event
    || "DayBoundary" in event,
  )
}
