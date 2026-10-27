import assert from "node:assert/strict";
import test from "node:test";
import { parsePlanBook } from "./schema/personal/plans.ts";
import { prepareSaveForWasm } from "../host/serde-normalize.ts";
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts";
import { validateDayEndCandidate } from "./day-end-candidate.ts";

const company = { plan_id: { owner: "17", sequence: "0" }, issuer: 3, stock: 7, status: "Funded", execution: null };
function book() {
  return {
    policy: { reverse_revision_threshold_bp: 2000, review_signal_delta_bp: 1000, review_price_change_bp: 200 },
    owner_sequences: [{ owner: "17", next_sequence: "1" }],
    plans: [{ IssuerRepurchase: company }],
  };
}

test("公司计划 tagged wire 保留独立来源并严格校验共同游标", () => {
  assert.deepEqual(parsePlanBook(book()), book());
  for (const plans of [[company], [{ Unknown: company }], [{ IssuerRepurchase: company, Investor: {} }], [{}]]) {
    assert.throws(() => parsePlanBook({ ...book(), plans }));
  }
  assert.throws(() => parsePlanBook({ ...book(), owner_sequences: [] }), /游标/);
  assert.throws(() => parsePlanBook({ ...book(), plans: [...book().plans, ...book().plans] }), /PlanId/);
});

test("公司执行腿拒绝非正价格数量和越界余量", () => {
  const maximum = { child_qty: 4_294_967_295, limit_price: "1234", expires_market_minute: 30, active_child_remaining_qty: 4_294_967_295 };
  assert.deepEqual(parsePlanBook({ ...book(), plans: [{ IssuerRepurchase: { ...company, execution: maximum } }] }).plans, [{ IssuerRepurchase: { ...company, execution: maximum } }]);
  for (const [child_qty, limit_price, active_child_remaining_qty] of [[0, "1234", null], [100, "0", null], [100, "-1", null], [100, "1234", 0], [100, "1234", 101]] as const) {
    const execution = { child_qty, limit_price, expires_market_minute: 30, active_child_remaining_qty };
    assert.throws(() => parsePlanBook({ ...book(), plans: [{ IssuerRepurchase: { ...company, execution } }] }));
  }
});

for (const remaining of [null, 4_294_967_296]) {
  test(`公司执行腿数量超过 u32 时明确拒绝：remaining=${remaining}`, () => {
    const execution = { child_qty: 4_294_967_296, limit_price: "1234", expires_market_minute: 30, active_child_remaining_qty: remaining };
    assert.throws(() => parsePlanBook({ ...book(), plans: [{ IssuerRepurchase: { ...company, execution } }] }));
  });
}

test("WASM 入站保持 tagged plans 数组与大整数 PlanId", () => {
  const fixture = currentSaveFixture();
  const plans = [{ IssuerRepurchase: { ...company, plan_id: { owner: "9007199254740993", sequence: "18446744073709551615" } } }];
  const prepared = prepareSaveForWasm({ ...fixture, plans: { ...book(), plans } } as unknown as Parameters<typeof prepareSaveForWasm>[0]) as { plans: { plans: unknown } };
  assert.ok(Array.isArray(prepared.plans.plans));
  assert.deepEqual(prepared.plans.plans, plans);
});

test("公共日终对公司 origin 同样拒绝任何 execution", () => {
  const fixture = withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02");
  const reference = { seq: 1, settledDate: "2030-01-01" };
  (fixture.snapshot as Record<string, unknown>).seq = 1;
  const candidate = { ...fixture, plans: book() };
  assert.doesNotThrow(() => validateDayEndCandidate(candidate, reference));
  assert.throws(() => validateDayEndCandidate({ ...candidate, plans: { ...book(), plans: [{ IssuerRepurchase: { ...company, execution: { child_qty: 100, limit_price: "1234", expires_market_minute: 30, active_child_remaining_qty: null } } }] } }, reference), /日终存档不能包含/);
});
