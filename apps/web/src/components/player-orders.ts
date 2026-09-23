import type { StrictSaveEnvelope } from "../save/schema/root.ts"
import type { Event } from "../types/generated/Event.ts"

export type PlayerWorkingOrder = {
  readonly id: number
  readonly code: string
  readonly side: "Buy" | "Sell"
  readonly price: number
  readonly remainingQty: number
  readonly venue: "auction" | "continuous"
  readonly frozen: "cash" | "shares"
}

type WorkingOrderSource = Pick<StrictSaveEnvelope, "auction_orders" | "resting_orders">

export function projectPlayerOrders(slot: WorkingOrderSource): readonly PlayerWorkingOrder[] {
  const orders: PlayerWorkingOrder[] = []
  for (const [code, auctionOrders] of Object.entries(slot.auction_orders)) {
    for (const order of auctionOrders) {
      if (order.owner !== 0) continue
      orders.push({
        id: order.arrival_seq,
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
      if (order.owner !== 0 || order.qty === 0) continue
      orders.push({
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

export function applyPlayerOrderFacts(
  current: readonly PlayerWorkingOrder[],
  facts: readonly PlayerOrderFact[],
  venue: PlayerWorkingOrder["venue"],
): readonly PlayerWorkingOrder[] {
  let orders = [...current]
  for (const { event } of facts) {
    if ("OrderAccepted" in event && event.OrderAccepted.account === 0) {
      const accepted = event.OrderAccepted
      orders = orders.filter((order) => order.id !== accepted.id || order.code !== accepted.code)
      orders.push({
        id: accepted.id,
        code: accepted.code,
        side: accepted.side,
        price: accepted.price,
        remainingQty: accepted.remaining_qty,
        venue,
        frozen: accepted.side === "Buy" ? "cash" : "shares",
      })
    } else if ("OrderCanceled" in event && event.OrderCanceled.account === 0) {
      const canceled = event.OrderCanceled
      orders = orders.filter((order) => order.id !== canceled.id || order.code !== canceled.code)
    } else if ("Trade" in event && event.Trade.maker === 0) {
      const trade = event.Trade
      const index = orders.findIndex((order) => order.venue === "continuous" && order.code === trade.code && order.price === trade.price)
      if (index >= 0) {
        const order = orders[index]!
        const remainingQty = Math.max(0, order.remainingQty - trade.qty)
        orders = remainingQty === 0
          ? orders.filter((_, orderIndex) => orderIndex !== index)
          : orders.map((candidate, orderIndex) => orderIndex === index ? { ...order, remainingQty } : candidate)
      }
    } else if ("AuctionCompleted" in event && event.AuctionCompleted.phase === "ClosingAuction") {
      orders = orders.filter((order) => order.venue !== "auction" || order.code !== event.AuctionCompleted.code)
    } else if ("DayBoundary" in event) {
      orders = []
    }
  }
  return orders.sort((left, right) => left.id - right.id)
}
