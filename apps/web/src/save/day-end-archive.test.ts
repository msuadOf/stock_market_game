import assert from "node:assert/strict";
import test from "node:test";
import * as candidate from "./day-end-candidate.ts";
import { parseSaveSlot } from "./save-schema.ts";
import { currentSaveFixture } from "./current-save-fixture.ts";
import type { StrictSaveEnvelope } from "./schema/root.ts";

function archive(): StrictSaveEnvelope {
  const slot = currentSaveFixture();
  const clock = slot.civil_clock as Record<string, unknown>;
  clock.current_date = "2030-01-02";
  clock.settled_through = "2030-01-01";
  return parseSaveSlot(slot);
}

function validate(slot: StrictSaveEnvelope): StrictSaveEnvelope {
  const validator = Reflect.get(candidate, "validateDayEndArchive");
  assert.equal(typeof validator, "function", "persistent archives must have a day-level boundary");
  return (validator as (value: StrictSaveEnvelope) => StrictSaveEnvelope)(slot);
}

test("a completed closed civil day is a valid daily archive without a trading tick", () => {
  const slot = archive();
  assert.equal(validate(slot), slot);
});

test("an unsettled or intraday snapshot cannot be loaded as a persistent archive", () => {
  const slot = archive();
  assert.throws(() => validate({ ...slot, civil_clock: { ...slot.civil_clock, settled_through: null } }), /日终/);
  assert.throws(() => validate({ ...slot, snapshot: { ...slot.snapshot, tick: 1 } }), /日内/);
});

test("daily archives cannot restore either live continuous or auction orders", () => {
  const slot = archive();
  const order = { id: 1, side: "Buy" as const, price: 1120, qty: 100, original_qty: 100, filled_qty: 0, filled_value: 0, owner: 0, seq: 0 };
  assert.throws(() => validate({ ...slot, resting_orders: { "600101": [order] } }), /活动委托/);
  assert.throws(() => validate({ ...slot, auction_orders: { "600101": [{ owner: 0, side: "Buy", limit: 1120, qty: 100, order_id: 1 }] } }), /活动委托/);
});

test("daily archives reject live resource envelopes and order expiry timers", () => {
  const slot = archive();
  assert.throws(() => validate({ ...slot, runtime_state: { ...slot.runtime_state, live_envelopes: [{ key: { account: 0, stock: "600101", order: 1, side: "Buy" }, charged: { commission: 0, stamp_tax: 0, transfer_fee: 0 } }] } }), /活动委托/);
  assert.throws(() => validate({ ...slot, npc_order_lifecycles: [{ account: 1, code: "600101", order_id: 1, placed_market_minute: 0, expires_market_minute: 1 }] }), /活动委托/);
});

test("daily archives cannot contain unprocessed player or NPC requests", () => {
  const slot = archive();
  const intent = { PlaceLimit: { code: "600101", side: "Buy" as const, price: { Fixed: 1120 }, qty: 100 } };
  assert.throws(() => validate({ ...slot, pending_player: [[0, intent]] }), /日内请求/);
  const batch = slot.pending_npc;
  assert.ok(batch);
  assert.throws(() => validate({ ...slot, pending_npc: { ...batch, intents: [[1, intent]] } }), /日内请求/);
});

test("daily archives cannot resume an unfinished intraday parent order", () => {
  const slot = archive();
  assert.throws(() => validate({ ...slot, parent_orders: { "1": { "600101": {
    code: "600101", side: "Buy", target_qty: 100, filled_qty: 0, child_qty: 100,
    active_child_order_id: null, limit_price: 1120, expires_market_minute: "500",
  } } } }), /活动委托/);
});
