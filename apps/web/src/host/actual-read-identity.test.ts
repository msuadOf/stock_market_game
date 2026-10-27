import assert from "node:assert/strict";
import test from "node:test";
import { prepareCatalogResolver } from "./catalog-resolver.ts";
import { parseInitialAllocation } from "./initial-allocation.ts";
import { parseNpcDecisionTrace } from "./npc-decision-trace.ts";
import { wireCurrentMinuteHistoryRequest } from "./current-minute-history.ts";

const identity = prepareCatalogResolver([{ id: 0, code: "600101" }], [{ id: 0, code: "C-600101", name: "实际公司", listed_stock: 0 }]);

test("实际单股只读请求发StockId0，initialAllocation与NPC诊断回相同证券", () => {
  const allocation = parseInitialAllocation({ stocks: [{ code: 0, float_shares: 0, unallocated_shares: 0, categories: ["Retail", "Inst", "Hot"].map(kind => ({ kind, shares: 0, account_count: 0, zero_holders: 0 })) }] }, ["600101"], identity);
  assert.equal(allocation.stocks[0].code, "600101");
  const trace = parseNpcDecisionTrace([{ account: 1, tick: "1", source_report_ids: [], expectation_method: null, plan_ids: [], plan_changes: [], budget_constraints: [], order_ids: [], codes: [0] }], identity);
  assert.equal(trace[0].codes[0], "600101");
  assert.equal(wireCurrentMinuteHistoryRequest({ code: "600101" }, identity).stock, 0);
});

test("真实WorkerHost只读请求按restore后实际子集发原StockId并拒绝旧响应", { timeout: 10000 }, async () => {
  const { createWorkerHost } = await import("./worker-host.ts");
  const { currentSaveFixture } = await import("../save/current-save-fixture.ts");
  const { snapshot, market, dailyCandle } = await import("./protocol-test-fixtures.ts");
  const original = Object.getOwnPropertyDescriptor(globalThis, "Worker");
  class QueryWorker extends EventTarget {
    static current: QueryWorker;
    sent: Record<string, unknown>[] = [];
    constructor() { super(); QueryWorker.current = this; }
    postMessage(value: Record<string, unknown>) { this.sent.push(value); }
    terminate() {}
    emit(data: unknown) { this.dispatchEvent(Object.assign(new Event("message"), { data })); }
  }
  Object.defineProperty(globalThis, "Worker", { configurable: true, value: QueryWorker });
  let host: Awaited<ReturnType<typeof createWorkerHost>> | undefined;
  try {
    const creating = createWorkerHost({} as never, 1n);
    const worker = QueryWorker.current;
    worker.emit({ type: "ingress", generation: 1, shared: false });
    worker.emit({ type: "baseline", generation: 1, snapshot: snapshot(0, 0), civilDate: "2030-01-02",
      stockCatalog: identity.stockCatalog, companyCatalog: identity.companyCatalog });
    host = await creating;
    const beforeIntent = host.submitIntent({ PlaceMarket: { code: "600101", side: "Buy", qty: 100 } });
    const beforeIntentRequest = worker.sent.at(-1)!;
    assert.deepEqual(beforeIntentRequest.intent, { PlaceMarket: { code: 0, side: "Buy", qty: 100 } });
    worker.emit({ type: "enqueued", generation: 1, requestId: beforeIntentRequest.requestId });
    await beforeIntent;
    const first = host.queryStockHistory("600101");
    const firstRequest = worker.sent.at(-1)!;
    assert.equal(firstRequest.code, 0);
    worker.emit({ type: "stockHistory", generation: 1, requestId: firstRequest.requestId,
      data: { code: 0, daily_candles: [], active_daily_candle: null } });
    assert.equal((await first).code, "600101");
    const delayed = host.queryStockHistory("600101");
    const delayedRequest = worker.sent.at(-1)!;
    const oldRejected = assert.rejects(delayed, /过期/);
    const loading = host.load(currentSaveFixture());
    const request = worker.sent.at(-1)!;
    const stockCatalog = [{ id: 0, code: "002156" }, { id: 1, code: "600101" }];
    worker.emit({ type: "ingress", generation: 2, shared: false });
    worker.emit({ type: "restored", generation: 1, nextGeneration: 2, requestId: request.requestId,
      snapshot: { ...snapshot(0, 0), markets: { "0": market(), "1": market() }, daily_candles: { "0": [], "1": [] }, active_daily_candles: { "0": dailyCandle(), "1": dailyCandle() } },
      stockCatalog, companyCatalog: stockCatalog.map(stock => ({ id: stock.id + 7, code: `C-${stock.code}`, name: stock.code, listed_stock: stock.id })), civilDate: "2030-02-02" });
    await loading;
    worker.emit({ type: "stockHistory", generation: 1, requestId: delayedRequest.requestId,
      data: { code: 0, daily_candles: [], active_daily_candle: null } });
    await oldRejected;
    const second = host.queryStockHistory("600101");
    const secondRequest = worker.sent.at(-1)!;
    assert.equal(secondRequest.generation, 2);
    assert.equal(secondRequest.code, 1);
    worker.emit({ type: "stockHistory", generation: 2, requestId: secondRequest.requestId,
      data: { code: 1, daily_candles: [], active_daily_candle: null } });
    assert.equal((await second).code, "600101");
    const afterIntent = host.submitIntent({ Cancel: { code: "600101", id: 7 } });
    const afterIntentRequest = worker.sent.at(-1)!;
    assert.deepEqual(afterIntentRequest.intent, { Cancel: { code: 1, id: 7 } });
    assert.equal(afterIntentRequest.generation, 2);
    worker.emit({ type: "enqueued", generation: 2, requestId: afterIntentRequest.requestId });
    await afterIntent;
    await assert.rejects(host.queryStockHistory("600610"), /未知股票代码/);
  } finally {
    await host?.dispose();
    if (original === undefined) delete (globalThis as { Worker?: unknown }).Worker;
    else Object.defineProperty(globalThis, "Worker", original);
  }
});
