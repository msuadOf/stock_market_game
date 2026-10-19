import assert from "node:assert/strict";
import test from "node:test";
import * as candidate from "./day-end-candidate.ts";
import { parseSaveSlot } from "./save-schema.ts";
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts";
import type { StrictSaveEnvelope } from "./schema/root.ts";

function archive(): StrictSaveEnvelope {
  return parseSaveSlot(withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02"));
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

test("daily archives cannot restore live unified-book orders, including legacy auction fields", () => {
  const slot = archive();
  const order = { id: 1, side: "Buy" as const, price: "1120", qty: 100, original_qty: 100, filled_qty: 0, filled_value: "0", owner: 0, seq: 0 };
  assert.throws(() => validate({ ...slot, resting_orders: { "600101": [order] } }), /活动委托/);
  // 统一 book 后存档不再有 auction_orders；旧字段在根校验即拒绝，不能绕过活动委托边界。
  assert.throws(
    () => parseSaveSlot({ ...slot, auction_orders: { "600101": [{ owner: 0, side: "Buy", limit: "1120", qty: 100, order_id: 1 }] } }),
    /auction_orders/,
  );
});

test("daily archives reject live resource envelopes and order expiry timers", () => {
  const slot = archive();
  assert.throws(() => validate({ ...slot, runtime_state: { ...slot.runtime_state, live_envelopes: [{ key: { account: 0, stock: 3, order: 1, side: "Buy" }, charged: { commission: "0", stamp_tax: "0", transfer_fee: "0" } }] } }), /活动委托/);
  assert.throws(() => validate({ ...slot, npc_order_lifecycles: [{ account: 1, code: 3, order_id: 1, placed_market_minute: 0, expires_market_minute: 1 }] }), /活动委托/);
});

test("daily archives cannot contain unprocessed player requests; legacy pending_npc saves are rejected", () => {
  const slot = archive();
  const intent = { PlaceLimit: { code: 3, side: "Buy" as const, price: { Fixed: "1120" }, qty: 100 } };
  const receipt = { owner: 0, intent, account_ordinal: "0", stock_ordinal: "0" };
  assert.throws(() => validate({ ...slot, pending_player: [receipt] }), /日内请求/);
  // T3 批四：pending_npc 载体删除——携带该键的存档是旧格式，根解析层显式
  // 拒绝（原「日终不得含 NPC 待处理批」负例随键删除升级为格式拒绝）。
  assert.throws(() => parseSaveSlot({ ...slot, pending_npc: { observed_tick: 0, observed_accounts: [1], intents: [], dependencies: [] } }), /旧格式存档/);
});

test("daily archives cannot resume an unfinished intraday execution leg", () => {
  const slot = archive();
  const plan = {
    plan_id: 1, account: 1, code: 0, direction: "Buy" as const, target: { ShareCount: 500 },
    filled_qty: 100, opinion: { signal_score_bp: 2_500, source: "Blended" as const },
    confidence_bp: 6_000, urgency: "Normal" as const, status: "Active" as const, version: 2,
    last_revision: null, last_resume: null, created_trading_day: 1, horizon_trading_days: 20,
    review: {
      min_signal_delta_bp: 1_000, min_price_change_bp: 200, last_review_signal_score_bp: 2_500,
      last_review_trading_day: 1, last_review_price: null, last_review_acquired_count: 0,
      last_review_resources: null,
    },
    active_child_order_id: 4,
    execution: { child_qty: 100, limit_price: "1120", expires_market_minute: 500, active_child_remaining_qty: 100 },
    last_event_trading_day: 3,
  }
  const withInflightChild = { ...slot, plans: { ...slot.plans, next_plan_seq: 2, plans: { "1": plan } } }
  assert.throws(() => validate(withInflightChild), /活动委托/)
  // 腿还在但没有在途子单：日终同样必须整体结束执行腿，不能带腿过日。
  const legWithoutChild = { ...withInflightChild, plans: { ...withInflightChild.plans, plans: {
    "1": { ...plan, active_child_order_id: null, execution: { ...plan.execution, active_child_remaining_qty: null } },
  } } }
  assert.throws(() => validate(legWithoutChild), /活动委托/)
  // 只有在途子单引用、没有执行腿的旧形态（引擎不会产出）同样拒绝。
  const childWithoutLeg = { ...withInflightChild, plans: { ...withInflightChild.plans, plans: {
    "1": { ...plan, execution: undefined },
  } } }
  assert.throws(() => validate(childWithoutLeg), /活动委托/)
});
