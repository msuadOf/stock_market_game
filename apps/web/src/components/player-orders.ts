import type { StrictSaveEnvelope } from "../save/schema/root.ts"
import type { CodedEvent as Event } from "../host/protocol/stock-identity.ts"
import type { Cents } from "../types/engine.ts"

export type PlayerWorkingOrder = {
  readonly id: number
  readonly code: string
  readonly side: "Buy" | "Sell"
  readonly price: Cents
  readonly remainingQty: number
  readonly venue: "Auction" | "Continuous"
  readonly frozen: "Cash" | "Shares"
}

type WorkingOrderSource = Pick<StrictSaveEnvelope, "resting_orders">

export function projectPlayerOrders(slot: WorkingOrderSource, venue: PlayerWorkingOrder["venue"]): readonly PlayerWorkingOrder[] {
  const orders: PlayerWorkingOrder[] = []
  // 统一 book 后存档不再携带独立竞价委托集合；投影 resting_orders 的调用方
  // 必须显式交出权威 venue，不从另一份委托集合或默认值猜测。
  for (const [code, restingOrders] of Object.entries(slot.resting_orders)) {
    for (const order of restingOrders) {
      if (order.owner !== "0" || order.qty === 0) continue
      orders.push({
        id: order.id,
        code,
        side: order.side,
        price: order.price,
        remainingQty: order.qty,
        venue,
        frozen: order.side === "Buy" ? "Cash" : "Shares",
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

export function playerOrderFactsRequireRefresh(facts: readonly PlayerOrderFact[]): boolean {
  return facts.some(({ event }) =>
    ("OrderAccepted" in event && event.OrderAccepted.account === "0")
    || ("OrderCanceled" in event && event.OrderCanceled.account === "0")
    || ("Trade" in event && (event.Trade.maker === "0" || event.Trade.taker === "0"))
    || "AuctionCompleted" in event
    || "DayBoundary" in event,
  )
}
