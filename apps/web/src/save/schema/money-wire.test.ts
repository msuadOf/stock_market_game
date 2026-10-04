import assert from "node:assert/strict";
import test from "node:test";
import { accountingAmount, integer, money, SaveSchemaError } from "./primitives.ts";
import { parseMoney, parseDepth, parseMarket } from "../../host/protocol/wire-values.ts";
import { ProtocolError } from "../../host/protocol/types.ts";
import { parseSaveSnapshot } from "./save-snapshot.ts";
import { parseSnapshot } from "./market.ts";
import { parseRetailExperienceState } from "./personal/experience.ts";
import { parseTradingPlan } from "./personal/plans.ts";

test("存档和宿主严格拒绝非规范 Money 并保留错误路径", { timeout: 10000 }, () => {
  for (const invalid of [1, "01", "-0", "+1", "1.00", "1e3", " 1", "1\n", "9223372036854775808"]) {
    assert.throws(() => money(invalid, "snapshot.cash"), (error) => error instanceof SaveSchemaError && error.path === "snapshot.cash");
    assert.throws(() => parseMoney(invalid, "snapshot.cash"), (error) => error instanceof ProtocolError && error.message.includes("snapshot.cash"));
  }
  assert.equal(money("-9223372036854775808", "cash"), "-9223372036854775808");
  assert.equal(parseMoney("9223372036854775807", "cash"), "9223372036854775807");
  assert.equal(integer(100, "qty"), 100);
  assert.equal(accountingAmount("100.00", "company"), "100.00");
});

test("盘口价格严格使用无损分字符串，股数仍是整数", { timeout: 10000 }, () => {
  assert.deepEqual(parseDepth([["9007199254740993", 100]], "bids"), [["9007199254740993", 100]]);
  assert.throws(() => parseDepth([[Number("9007199254740993"), 100]], "bids"), /bids\[0\]\[0\]/);
  const market = { last_price: "9007199254740993", last_close: "9007199254740992", best_bid: null, best_ask: null, bids: [], asks: [] };
  assert.deepEqual(parseMarket(market, "market"), market);
  assert.throws(() => parseMarket({ ...market, last_close: 1 }, "market"), /last_close/);
});

test("公共存档资金与成本在安全整数外精确保留", { timeout: 10000 }, () => {
  const snapshot = {
    seq: 0, tick: 0, markets: {}, daily_candles: {}, active_daily_candles: {},
    accounts: { "0": { cash: "9007199254740993", positions: { "600000": { qty: 100, t1_locked: 0, invested_cents: "9007199254740993", recovered_cents: "2" } } } },
  };
  assert.equal(parseSaveSnapshot(snapshot, "snapshot").accounts[0]!.cash, "9007199254740993");
  assert.throws(() => parseSaveSnapshot({ ...snapshot, accounts: { "0": { cash: "-1", positions: {} } } }, "snapshot"), /cash.*不能为负数/);
});

test("运行快照 PositionSnap 成本严格接受大额字符串并拒绝旧数字", { timeout: 10000 }, () => {
  const position = { qty: 100, t1_locked: 0, invested_cents: "9007199254740993", recovered_cents: "9223372036854775807" };
  const snapshot = {
    seq: 0, tick: 0, day: 0, phase: "Continuous", markets: {}, daily_candles: {}, active_daily_candles: {},
    accounts: { "0": { cash: "10", reserved_cash: "0", reserved_sell_qty: {}, positions: { "600000": position } } },
  };
  assert.deepEqual(parseSnapshot(snapshot, "snapshot").accounts[0]!.positions["600000"], position);
  for (const field of ["invested_cents", "recovered_cents"]) {
    const invalid = structuredClone(snapshot);
    Object.assign(invalid.accounts[0].positions["600000"], { [field]: 1 });
    assert.throws(() => parseSnapshot(invalid, "snapshot"), new RegExp(field));
  }
});

test("NPC 经历金额严格使用 Money 而不误改失败计数", { timeout: 10000 }, () => {
  const experience = { reference_equity: "9007199254740993", peak_equity: "9223372036854775807", consecutive_failed_buys: 2, stocks: {} };
  assert.deepEqual(parseRetailExperienceState(experience, "experience"), experience);
  assert.throws(() => parseRetailExperienceState({ ...experience, reference_equity: 1 }, "experience"), /reference_equity/);
});

test("计划历史资源按整数金额而非字符串字典序校验占用", { timeout: 10000 }, () => {
  const plan = {
    plan_id: 1, account: 1, code: "600000", direction: "Buy", target: { ShareCount: 100 }, filled_qty: 0,
    opinion: { signal_score_bp: 1, source: "Fundamental" }, confidence_bp: 1, urgency: "Normal", status: "Active",
    version: 1, last_revision: null, last_resume: null, created_trading_day: 0, horizon_trading_days: 1,
    active_child_order_id: null, last_event_trading_day: 0,
    review: {
      min_signal_delta_bp: 1, min_price_change_bp: 1, last_review_signal_score_bp: 1, last_review_trading_day: 0,
      last_review_price: null, last_review_acquired_count: 0,
      last_review_resources: { cash: "10", frozen_cash: "9", held_qty: 0, t1_locked: 0 },
    },
  };
  assert.equal(parseTradingPlan(plan, "plan").review.last_review_resources!.cash, "10");
  assert.throws(() => parseTradingPlan({ ...plan, review: { ...plan.review, last_review_resources: { cash: "9", frozen_cash: "10", held_qty: 0, t1_locked: 0 } } }, "plan"), /资源事实不一致/);
  assert.throws(() => parseTradingPlan({ ...plan, review: { ...plan.review, last_review_resources: { cash: "9", frozen_cash: "-1", held_qty: 0, t1_locked: 0 } } }, "plan"), /资源事实不一致/);
});
