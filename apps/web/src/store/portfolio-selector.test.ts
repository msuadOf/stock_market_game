import assert from "node:assert/strict";
import test from "node:test";
import type { RootState } from "./store.ts";
import type { AccountSnap, MarketSnap } from "../types/engine.ts";
import { portfolioInputEqual, selectPortfolioInput } from "../app/portfolio-selector.ts";

const account = {
  cash: "10000",
  reserved_cash: "0",
  reserved_sell_qty: {},
  positions: { HELD: { qty: 100, t1_locked: 0, invested_cents: "1000", recovered_cents: "0" } },
} as AccountSnap;
const market = (lastPrice: string) => ({ last_price: lastPrice } as MarketSnap);
// B3 远程登录：组合选择器经成员选择器取账户——本地局 remoteMembership 域
// 形如 localMembership（账户 0）。
const state = (held: string, unrelated: string) => ({ remoteMembership: { accountId: "0", canControl: true, needsRejoin: false, remote: false }, snapshot: { snapshot: { accounts: { "0": account }, markets: { HELD: market(held), OTHER: market(unrelated) } } } } as unknown as RootState);

test("portfolio selector ignores unrelated market ticks but observes held-price changes", () => {
  const before = selectPortfolioInput(state("100", "200"));
  assert.equal(portfolioInputEqual(before, selectPortfolioInput(state("100", "201"))), true);
  assert.equal(portfolioInputEqual(before, selectPortfolioInput(state("101", "200"))), false);
});
