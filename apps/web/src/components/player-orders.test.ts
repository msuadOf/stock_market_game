import assert from "node:assert/strict"
import test from "node:test"

import { PlayerOrderRefreshGate, playerOrderFactsRequireRefresh, projectPlayerOrders } from "./player-orders.ts"

test("a save response started before loading cannot replace loaded orders", async () => {
  const gate = new PlayerOrderRefreshGate()
  const pending = gate.next()
  let resolveSave: (value: number) => void = () => { throw new Error("save resolver was not installed") }
  const oldSave = new Promise<number>((resolve) => { resolveSave = resolve })
  const displayed = [8]
  gate.invalidate()
  resolveSave(7)
  const staleOrder = await oldSave
  if (gate.isCurrent(pending)) displayed.splice(0, displayed.length, staleOrder)
  assert.deepEqual(displayed, [8])
})

test("post-load refresh supersedes a baseline refresh and the loaded snapshot", async () => {
  const gate = new PlayerOrderRefreshGate()
  const baselineRequest = gate.next()
  const displayed = [8]
  gate.invalidate()
  const postLoadRequest = gate.next()
  if (gate.isCurrent(baselineRequest)) displayed.splice(0, displayed.length, 7)
  if (gate.isCurrent(postLoadRequest)) displayed.splice(0, displayed.length, 9)
  assert.deepEqual(displayed, [9])
})

test("requires authoritative refresh for same-tick acceptance and trade regardless of fact order", () => {
  const accepted = { event: { OrderAccepted: { seq: 2, account: 0, code: "600101", id: 7, side: "Buy" as const, price: 1_008, remaining_qty: 200 } } }
  const trade = { event: { Trade: { seq: 1, code: "600101", price: 1_008, qty: 100, maker: 0, taker: 4 } } }
  assert.equal(playerOrderFactsRequireRefresh([trade, accepted]), true)
  assert.equal(playerOrderFactsRequireRefresh([accepted, trade]), true)
})

test("keeps same-price orders distinct when projected from the authoritative save", () => {
  const orders = projectPlayerOrders({
    auction_orders: {},
    resting_orders: { "600101": [
      { id: 7, owner: 0, side: "Buy", price: 1_008, qty: 100, original_qty: 200, filled_qty: 100, filled_value: 100_800, seq: 1 },
      { id: 8, owner: 0, side: "Buy", price: 1_008, qty: 200, original_qty: 200, filled_qty: 0, filled_value: 0, seq: 2 },
    ] },
  })
  assert.deepEqual(orders.map(({ id, remainingQty }) => [id, remainingQty]), [[7, 100], [8, 200]])
})

test("projects only the player's authoritative auction and continuous orders", () => {
  const orders = projectPlayerOrders({
    auction_orders: {
      "600101": [
        { owner: 0, side: "Buy", limit: 1_105, qty: 200, order_id: 7 },
        { owner: 3, side: "Sell", limit: 1_120, qty: 100, order_id: 8 },
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
    auction_orders: { "600101": [{ owner: 0, side: "Sell" as const, limit: 1_100, qty: 100, order_id: 20 }] },
    resting_orders: { "600101": [{ id: 3, owner: 0, side: "Buy" as const, price: 1_000, qty: 100, original_qty: 100, filled_qty: 0, filled_value: 0, seq: 2 }] },
  }
  const before = structuredClone(slot)

  assert.deepEqual(projectPlayerOrders(slot).map((order) => order.id), [3, 20])
  assert.deepEqual(slot, before)
})

test("refreshes after player order lifecycle changes and auction transitions", () => {
  assert.equal(playerOrderFactsRequireRefresh([
    { event: { OrderCanceled: { seq: 2, account: 0, code: "600101", id: 7, remaining_qty: 200 } } },
  ]), true)
  assert.equal(playerOrderFactsRequireRefresh([
    { event: { Trade: { seq: 3, code: "600101", price: 1_120, qty: 100, maker: 0, taker: 4 } } },
  ]), true)
  assert.equal(playerOrderFactsRequireRefresh([
    { event: { AuctionCompleted: { seq: 4, tick: 6, phase: "CallAuction", code: "600101", clearing_price: null, matched_volume: 0 } } },
  ]), true)
  assert.equal(playerOrderFactsRequireRefresh([
    { event: { AuctionCompleted: { seq: 5, tick: 30, phase: "ClosingAuction", code: "600101", clearing_price: null, matched_volume: 0 } } },
  ]), true)
  assert.equal(playerOrderFactsRequireRefresh([{ event: { DayBoundary: { seq: 6, day: 2, closed_daily_candles: {} } } }]), true)
  assert.equal(playerOrderFactsRequireRefresh([]), false)
})
