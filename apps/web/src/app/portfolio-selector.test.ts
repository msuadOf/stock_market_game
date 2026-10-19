import assert from "node:assert/strict";
import test from "node:test";
import { selectPortfolioInput } from "./portfolio-selector.ts";
import { parseProtocolSnapshot } from "../host/protocol/index.ts";
import { ProtocolError } from "../host/protocol/types.ts";
import type { RootState } from "../store/store.ts";

function snapshot(markets: Record<string, unknown>) {
  return { seq: 0, tick: 0, day: 0, phase: "Continuous", markets,
    accounts: { "0": { cash: "1000", reserved_cash: "0", reserved_sell_qty: {}, positions: { "3": { qty: 200, t1_locked: 0, invested_cents: "200100", recovered_cents: "0" } } } }, daily_candles: {}, active_daily_candles: {} };
}

test("审计G67：baseline拒绝没有行情的持仓引用，估值不能当零价", () => {
  const invalid = snapshot({});
  assert.throws(() => parseProtocolSnapshot(invalid), /缺少行情/);
  assert.throws(() => selectPortfolioInput({ remoteMembership: { accountId: "0", canControl: true, needsRejoin: false, remote: false }, snapshot: { snapshot: invalid } } as unknown as RootState), /缺少行情/);
  const valid = parseProtocolSnapshot(snapshot({ "3": { last_price: "1001", last_close: "1000", best_bid: null, best_ask: null, bids: [], asks: [] } }));
  assert.equal(selectPortfolioInput({ remoteMembership: { accountId: "0", canControl: true, needsRejoin: false, remote: false }, snapshot: { snapshot: valid } } as unknown as RootState).heldPrices["600101"], "1001");
});

test("审计G67：非玩家和零股持仓引用同样校验，继承属性不是行情", () => {
  for (const quantity of [0, 200]) {
    const invalid = snapshot({});
    const foreign = { ...invalid, accounts: { "7": { ...invalid.accounts["0"], positions: { "3": { ...invalid.accounts["0"].positions["3"], qty: quantity } } } } };
    assert.throws(() => parseProtocolSnapshot(foreign), error => error instanceof ProtocolError && error.where === "Snapshot.accounts.7.positions.600101");
  }
  const invalid = snapshot(Object.create({ "600101": { last_price: "1001", last_close: "1000", best_bid: null, best_ask: null, bids: [], asks: [] } }));
  assert.throws(() => parseProtocolSnapshot(invalid), /缺少行情/);
  assert.throws(() => selectPortfolioInput({ remoteMembership: { accountId: "0", canControl: true, needsRejoin: false, remote: false }, snapshot: { snapshot: invalid } } as unknown as RootState), /缺少行情/);
});
