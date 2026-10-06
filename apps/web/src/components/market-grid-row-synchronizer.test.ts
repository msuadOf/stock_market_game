import assert from "node:assert/strict";
import test from "node:test";
import { buildMarketRows, type MarketRowTransaction } from "./market-grid-rows.ts";
import { MarketGridRowSynchronizer } from "./market-grid-row-synchronizer.ts";

const market = (last_price: number) => ({ last_price: String(last_price), last_close: "1000", cash_ex_reference_pending_trade: false, day_market_activity: false, last_cash_ex_reference: null, best_bid: null, best_ask: null, bids: [], asks: [] });
const rows = (price: number) => buildMarketRows({ AAA: market(price), BBB: market(2000) }, ["AAA", "BBB"]);

test("成员或顺序变化刷新有序 rowData，先清空旧异步事务；报价更新仍只提交对应行", { timeout: 10000 }, () => {
  const initial = rows(1000);
  const calls: unknown[] = [];
  const owner = new MarketGridRowSynchronizer(initial);
  owner.attach({ applyTransactionAsync: transaction => calls.push(transaction), flushAsyncTransactions: () => calls.push("flush"), setGridOption: (key, value) => calls.push([key, value]) }, initial);
  owner.updateLatest([initial[1]]);
  owner.updateLatest(initial);
  assert.deepEqual(calls, ["flush", ["rowData", [initial[1]]], "flush", ["rowData", initial]]);
  calls.length = 0;
  const changed = buildMarketRows({ AAA: market(1001), BBB: initial[1]._source }, ["AAA", "BBB"], initial);
  owner.updateLatest(changed);
  assert.deepEqual(calls, [{ add: [], remove: [], update: [changed[0]] }]);
});

test("迟绑定只提交最新目标，连续异步提交以前一次目标为基准", { timeout: 10000 }, () => {
  const initial = rows(1000);
  const first = buildMarketRows({ AAA: market(1001), BBB: initial[1]._source }, ["AAA", "BBB"], initial);
  const latest = buildMarketRows({ AAA: market(1002), BBB: initial[1]._source }, ["AAA", "BBB"], first);
  const transactions: MarketRowTransaction[] = [];
  const replacements: unknown[] = [];
  const owner = new MarketGridRowSynchronizer(initial);
  owner.updateLatest(first);
  owner.recordLatest(latest);
  assert.equal(transactions.length, 0);
  owner.attach({ applyTransactionAsync: (transaction) => { transactions.push(transaction); }, flushAsyncTransactions() {}, setGridOption: (key, value) => { replacements.push([key, value]); } }, initial);
  assert.equal(transactions.length, 1);
  assert.deepEqual(transactions[0].update, [latest[0]]);
  assert.equal(latest[1], initial[1]);
  owner.updateLatest(latest);
  owner.updateLatest([latest[0]]);
  owner.updateLatest([]);
  assert.equal(transactions.length, 1);
  assert.deepEqual(replacements, [["rowData", [latest[0]]], ["rowData", []]]);
});

test("首次 ready 与 effect 不重复添加，重新 attach 使用新 grid 初始行", { timeout: 10000 }, () => {
  const initial = rows(1000);
  const owner = new MarketGridRowSynchronizer(initial);
  const old: unknown[] = [];
  owner.attach({ applyTransactionAsync: transaction => { old.push(transaction); }, flushAsyncTransactions() {}, setGridOption: (key, value) => { old.push([key, value]); } }, initial);
  owner.updateLatest(initial);
  assert.equal(old.length, 0);
  owner.dispose();
  owner.updateLatest([initial[0]]);
  const fresh: unknown[] = [];
  owner.attach({ applyTransactionAsync: transaction => { fresh.push(transaction); }, flushAsyncTransactions() {}, setGridOption: (key, value) => { fresh.push([key, value]); } }, []);
  assert.equal(fresh.length, 1);
  assert.deepEqual(fresh[0], ["rowData", [initial[0]]]);
  assert.equal(old.length, 0);
});

test("render 登记不提交，API 抛错时不前移 submitted target", { timeout: 10000 }, () => {
  const initial = rows(1000);
  const latest = rows(1001);
  const owner = new MarketGridRowSynchronizer(initial);
  let fail = true;
  const transactions: MarketRowTransaction[] = [];
  owner.attach({ applyTransactionAsync: (transaction) => {
    if (fail) throw new Error("提交失败");
    transactions.push(transaction);
  }, flushAsyncTransactions() {}, setGridOption() { throw new Error("本例不能替换行"); } }, initial);
  owner.recordLatest(latest);
  assert.equal(transactions.length, 0);
  assert.throws(() => owner.updateLatest(latest), /提交失败/);
  fail = false;
  owner.updateLatest(latest);
  assert.deepEqual(transactions[0].update, latest);
});
