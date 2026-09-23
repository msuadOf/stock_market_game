import assert from "node:assert/strict"
import test from "node:test"

import { applyPlayerOrderFacts, projectPlayerOrders } from "./player-orders.ts"

test("projects only the player's authoritative auction and continuous orders", () => {
  const orders = projectPlayerOrders({
    auction_orders: {
      "600101": [
        { owner: 0, side: "Buy", limit: 1_105, qty: 200, arrival_seq: 7 },
        { owner: 3, side: "Sell", limit: 1_120, qty: 100, arrival_seq: 8 },
      ],
    },
    resting_orders: {
      "002156": [
        { id: 11, owner: 0, side: "Sell", price: 2_700, qty: 100, original_qty: 200, filled_qty: 100, filled_value: 270_000, seq: 19 },
      ],
    },
  })

  assert.deepEqual(orders, [
    { id: 7, code: "600101", side: "Buy", price: 1_105, remainingQty: 200, venue: "auction", frozen: "cash" },
    { id: 11, code: "002156", side: "Sell", price: 2_700, remainingQty: 100, venue: "continuous", frozen: "shares" },
  ])
})

test("sorts player orders by stable order id without mutating the save payload", () => {
  const slot = {
    auction_orders: { "600101": [{ owner: 0, side: "Sell" as const, limit: 1_100, qty: 100, arrival_seq: 20 }] },
    resting_orders: { "600101": [{ id: 3, owner: 0, side: "Buy" as const, price: 1_000, qty: 100, original_qty: 100, filled_qty: 0, filled_value: 0, seq: 2 }] },
  }
  const before = structuredClone(slot)

  assert.deepEqual(projectPlayerOrders(slot).map((order) => order.id), [3, 20])
  assert.deepEqual(slot, before)
})

test("applies authoritative player order lifecycle facts without a save round-trip", () => {
  const accepted = applyPlayerOrderFacts([], [
    { event: { OrderAccepted: { seq: 1, account: 0, code: "600101", id: 7, side: "Buy", price: 1_008, remaining_qty: 200 } } },
  ], "auction")
  assert.deepEqual(accepted, [
    { id: 7, code: "600101", side: "Buy", price: 1_008, remainingQty: 200, venue: "auction", frozen: "cash" },
  ])

  const canceled = applyPlayerOrderFacts(accepted, [
    { event: { OrderCanceled: { seq: 2, account: 0, code: "600101", id: 7, remaining_qty: 200 } } },
  ], "auction")
  assert.deepEqual(canceled, [])
})

test("reduces a player's resting maker order while opening completion preserves orders until authoritative refresh", () => {
  const continuous = [{ id: 9, code: "600101", side: "Sell" as const, price: 1_120, remainingQty: 200, venue: "continuous" as const, frozen: "shares" as const }]
  const partial = applyPlayerOrderFacts(continuous, [
    { event: { Trade: { seq: 3, code: "600101", price: 1_120, qty: 100, maker: 0, taker: 4 } } },
  ], "continuous")
  assert.equal(partial[0]?.remainingQty, 100)

  const auction = [{ ...continuous[0]!, venue: "auction" as const }]
  const opening = applyPlayerOrderFacts(auction, [
    { event: { AuctionCompleted: { seq: 4, tick: 6, phase: "CallAuction", code: "600101", clearing_price: null, matched_volume: 0 } } },
  ], "auction")
  assert.deepEqual(opening, auction)

  const cleared = applyPlayerOrderFacts(opening, [
    { event: { AuctionCompleted: { seq: 5, tick: 30, phase: "ClosingAuction", code: "600101", clearing_price: null, matched_volume: 0 } } },
  ], "auction")
  assert.deepEqual(cleared, [])
})
