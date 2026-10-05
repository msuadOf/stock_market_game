import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import * as candidate from "./day-end-candidate.ts";
import { parseSaveSlot } from "./save-schema.ts";
import { representativeCurrentSaveFixture } from "./representative-save-test-fixture.ts";
import type { StrictSaveEnvelope } from "./schema/root.ts";

function archive(): StrictSaveEnvelope {
  return parseSaveSlot(representativeCurrentSaveFixture());
}

function closedDayArchive(): StrictSaveEnvelope {
  return parseSaveSlot(JSON.parse(readFileSync(new URL("./fixtures/current-closed-day-save.json", import.meta.url), "utf8")));
}

function validate(slot: StrictSaveEnvelope): StrictSaveEnvelope {
  const validator = Reflect.get(candidate, "validateDayEndArchive");
  assert.equal(typeof validator, "function", "persistent archives must have a day-level boundary");
  return (validator as (value: StrictSaveEnvelope) => StrictSaveEnvelope)(slot);
}

test("a completed closed civil day is a valid daily archive without a trading tick", () => {
  const slot = closedDayArchive();
  assert.equal(slot.snapshot.tick, 0);
  assert.equal(slot.setup.start_date, "2026-01-01");
  assert.equal(slot.civil_clock.current_date, "2026-01-02");
  assert.equal(slot.civil_clock.settled_through, "2026-01-01");
  assert.equal(slot.company_system.implementation.state.advanced_through, "2026-01-01");
  assert.deepEqual(slot.retained_market_history, [{ date: "2026-01-01", securities: { "600101": { status: "Closed", bars: [] } } }]);
  assert.equal(validate(slot), slot);
});

test("a completed civil day requires its explicit archive coverage", { timeout: 10000 }, () => {
  const slot = closedDayArchive();
  assert.throws(() => parseSaveSlot({ ...slot, retained_market_history: [] }), /未完整覆盖/);
  assert.throws(() => parseSaveSlot({ ...slot, retained_market_history: [{ date: "2026-01-01", securities: {} }] }), /600101/);
});

test("daily archives reject company advancement that disagrees with the saved natural date", { timeout: 10000 }, () => {
  const slot = closedDayArchive();
  assert.throws(() => parseSaveSlot({ ...slot, company_system: { ...slot.company_system, implementation: { ...slot.company_system.implementation, state: { ...slot.company_system.implementation.state, advanced_through: "2025-12-31" } } } }), /推进日期/);
});

test("an unsettled or intraday snapshot cannot be loaded as a persistent archive", () => {
  const slot = archive();
  assert.throws(() => validate({ ...slot, civil_clock: { ...slot.civil_clock, settled_through: null } }), /日终/);
  assert.throws(() => validate({ ...slot, snapshot: { ...slot.snapshot, tick: 1 } }), /日内/);
});

test("daily archives cannot restore either live continuous or auction orders", () => {
  const slot = archive();
  const order = { id: 1, side: "Buy" as const, price: "1120", qty: 100, original_qty: 100, filled_qty: 0, filled_value: "0", owner: "0", seq: 0 };
  assert.throws(() => validate({ ...slot, resting_orders: { "600101": [order] } }), /活动委托/);
  assert.throws(() => validate({ ...slot, auction_orders: { "600101": [{ owner: "0", side: "Buy", limit: "1120", qty: 100, order_id: 1 }] } }), /活动委托/);
});

test("daily archives reject live resource envelopes and order expiry timers", () => {
  const slot = archive();
  assert.throws(() => validate({ ...slot, runtime_state: { ...slot.runtime_state, live_envelopes: [{ key: { account: "0", stock: "600101", order: 1, side: "Buy" }, charged: { commission: "0", stamp_tax: "0", transfer_fee: "0" } }] } }), /活动委托/);
  assert.throws(() => validate({ ...slot, npc_order_lifecycles: [{ account: "1", code: "600101", order_id: 1, placed_market_minute: 0, expires_market_minute: 1 }] }), /活动委托/);
});

test("daily archives cannot contain unprocessed player or NPC requests", () => {
  const slot = archive();
  const intent = { PlaceLimit: { code: "600101", side: "Buy" as const, price: { Fixed: "1120" }, qty: 100 } };
  const receipt = { owner: "0", intent, account_ordinal: "0", stock_ordinal: "0" };
  assert.throws(() => validate({ ...slot, pending_player: [receipt] }), /日内请求/);
  assert.equal(slot.pending_npc, null);
  assert.throws(() => validate({ ...slot, pending_npc: { observed_tick: slot.snapshot.tick, observed_accounts: ["1"], dependencies: [], intents: [{ ...receipt, owner: "1" }] } }), /日内请求/);
});

test("daily archives cannot resume an unfinished intraday parent order", () => {
  const slot = archive();
  assert.throws(() => validate({ ...slot, parent_orders: { "1": { "600101": {
    code: "600101", side: "Buy", target_qty: 100, filled_qty: 0, child_qty: 100,
    active_child_order_id: null, limit_price: "1120", expires_market_minute: "500",
  } } } }), /活动委托/);
});
