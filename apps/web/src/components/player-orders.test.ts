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
  const accepted = { event: { OrderAccepted: { seq: 2, account: 0, code: "600101", id: 7, side: "Buy" as const, price: "1008", remaining_qty: 200 } } }
  const trade = { event: { Trade: { seq: 1, code: "600101", price: "1008", qty: 100, maker: 0, taker: 4 } } }
  assert.equal(playerOrderFactsRequireRefresh([trade, accepted]), true)
  assert.equal(playerOrderFactsRequireRefresh([accepted, trade]), true)
})

test("keeps same-price orders distinct when projected from the authoritative save", () => {
  const orders = projectPlayerOrders({
    resting_orders: { "600101": [
      { id: 7, owner: 0, side: "Buy", price: "1008", qty: 100, original_qty: 200, filled_qty: 100, filled_value: "100800", seq: 1 },
      { id: 8, owner: 0, side: "Buy", price: "1008", qty: 200, original_qty: 200, filled_qty: 0, filled_value: "0", seq: 2 },
    ] },
  }, "Continuous")
  assert.deepEqual(orders.map(({ id, remainingQty }) => [id, remainingQty]), [[7, 100], [8, 200]])
})

test("projects only the player's authoritative unified-book orders", () => {
  // 存档 wire 的委托 map 键是 StockId 字符串（ADR 0041）：3=600101、1=002156；
  // 投影结果 code 原样透传该键。统一 book 后存档只有 resting_orders。
  const auctionVenue = projectPlayerOrders({
    resting_orders: {
      "3": [
        { id: 7, owner: 0, side: "Buy", price: "1105", qty: 200, original_qty: 200, filled_qty: 0, filled_value: "0", seq: 1 },
        { id: 8, owner: 3, side: "Sell", price: "1120", qty: 100, original_qty: 100, filled_qty: 0, filled_value: "0", seq: 2 },
      ],
    },
  }, "Auction")
  assert.deepEqual(auctionVenue, [
    { id: 7, code: "3", side: "Buy", price: "1105", remainingQty: 200, venue: "Auction", frozen: "Cash" },
  ])
  const continuousVenue = projectPlayerOrders({
    resting_orders: {
      "1": [
        { id: 11, owner: 0, side: "Sell", price: "2700", qty: 100, original_qty: 200, filled_qty: 100, filled_value: "270000", seq: 19 },
      ],
    },
  }, "Continuous")
  assert.deepEqual(continuousVenue, [
    { id: 11, code: "1", side: "Sell", price: "2700", remainingQty: 100, venue: "Continuous", frozen: "Shares" },
  ])
})

test("sorts player orders by stable order id without mutating the save payload", () => {
  const slot = {
    resting_orders: {
      "600101": [
        { id: 20, owner: 0, side: "Sell" as const, price: "1100", qty: 100, original_qty: 100, filled_qty: 0, filled_value: "0", seq: 3 },
        { id: 3, owner: 0, side: "Buy" as const, price: "1000", qty: 100, original_qty: 100, filled_qty: 0, filled_value: "0", seq: 2 },
      ],
    },
  }
  const before = structuredClone(slot)

  assert.deepEqual(projectPlayerOrders(slot, "Continuous").map((order) => order.id), [3, 20])
  assert.deepEqual(slot, before)
})

test("refreshes after player order lifecycle changes and auction transitions", () => {
  assert.equal(playerOrderFactsRequireRefresh([
    { event: { OrderCanceled: { seq: 2, account: 0, code: "600101", id: 7, remaining_qty: 200 } } },
  ]), true)
  assert.equal(playerOrderFactsRequireRefresh([
    { event: { Trade: { seq: 3, code: "600101", price: "1120", qty: 100, maker: 0, taker: 4 } } },
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
