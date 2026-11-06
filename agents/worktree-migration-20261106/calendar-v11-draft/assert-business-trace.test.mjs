// 合成期望trace仅验证独立oracle会拒绝业务污染，不冒engine、native或WASM实际结果。
import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { assertBusinessTrace, canonicalBusinessTrace } from "./assert-business-trace.mjs";

const clone = structuredClone;
const bar = (price) => ({ minute_of_day: 570, phase: "Continuous", open: String(price), high: String(price), low: String(price), close: String(price), volume_shares: "100", turnover_cents: String(price * 100), trade_count: "1" });
const candle = (price, volume = 100) => ({ open: String(price), high: String(price), low: String(price), close: String(price), volume, trade_stats: { turnover_cents: String(price * volume), trade_count: volume === 0 ? 0 : 1 } });
const position = (qty, locked = 0) => ({ qty, t1_locked: locked });
const account = (cash, openQty, closedQty = 0, locked = 0) => ({ cash: String(cash), positions: { ...(openQty ? { 0: position(openQty, locked) } : {}), ...(closedQty ? { 1: position(closedQty) } : {}) }, reserved_cash: "0", reserved_sell_qty: {} });
const confirmation = (side, date, price, receipt, tax) => ({ side, stock: 0, civil_date: date, quantity_shares: 100, price_cents: String(price), gross_cents: String(price * 100), receipt_id: String(receipt), actual_fees: { commission: "500", transfer_fee: "1", stamp_tax: String(tax) } });
const history = (stock, date, price) => ({ date, availability: price === null ? "Closed" : stock === 1 ? "NoTrades" : "Traded", daily_candle: price === null ? null : candle(price, stock === 1 ? 0 : 100), bars: price === null || stock === 1 ? [] : [bar(price)] });

function legalTrace() {
  const ticks = [1, 2, 2, 3, 4, 4], days = [0, 1, 1, 1, 2, 2];
  const dates = ["2030-01-02", "2030-01-02", "2030-01-03", "2030-01-03", "2030-01-03", "2030-01-04"];
  const confirmations = (last = false) => ({ 0: [confirmation("Buy", "2030-01-02", 900, 1, 0), ...(last ? [confirmation("Buy", "2030-01-03", 810, 3, 0)] : [])], 1: [confirmation("Sell", "2030-01-02", 900, 0, 45), ...(last ? [confirmation("Sell", "2030-01-03", 810, 2, 40)] : [])] });
  const markets = { 0: { price: "1000" }, 1: { price: "1000" } };
  const initialAccounts = { 0: account(1000000, 0), 1: account(1000000, 1000, 1000) };
  const initial = { civilDate: "2030-01-02", snapshot: { tick: 0, day: 0, markets: clone(markets) }, workingOrders: [], dayEndAccounts: clone(initialAccounts), dayEndConfirmations: { 0: [], 1: [] } };
  const frames = ticks.map((tick, index) => {
    const lastDay = index >= 3, ended = index === 1 || index === 4, civil = index === 2 || index === 5;
    const stockSet = lastDay ? [0, 1] : [0];
    const trade = { code: 0, maker: "1", taker: "0", qty: 100, price: lastDay ? "810" : "900", seq: lastDay ? 1 : 0 };
    const facts = stockSet.map((code) => ({ event: { PriceTick: { code } } }));
    if (index === 0 || index === 3) facts.push({ event: { Trade: trade } });
    if (index === 0) facts.push({ event: { IntentRejected: { code: 1, reason: "ExchangeClosed" } } });
    const closed_daily_candles = ended ? Object.fromEntries(stockSet.map((stock) => [stock, {}])) : {};
    if (ended) facts.push({ event: { DayBoundary: { closed_daily_candles: clone(closed_daily_candles) } } });
    const update = civil ? { CivilUpdate: { tick, civil_date: dates[index] } } : { TickBatch: { frames: [{ tick, facts, timeseries_payload: { active_daily_candles: {}, continuous_points: Object.fromEntries(stockSet.map((stock) => [stock, {}])), auction_points: {}, closed_daily_candles } }] } };
    const accounts = { 0: account(lastDay ? 827998 : 909499, lastDay ? 200 : 100, 0, index === 0 || index === 3 ? 100 : 0), 1: account(lastDay ? 1169913 : 1089454, lastDay ? 800 : 900, 1000) };
    const queryDate = index < 2 ? "2030-01-02" : index < 5 ? "2030-01-03" : "2030-01-04";
    const currentMinute = Object.fromEntries([0, 1].map((stock) => [stock, { stock, date: queryDate, live: true, trading: stock === 0 || lastDay, phase: stock === 1 && !lastDay ? "Closed" : ended ? "AfterClose" : "Continuous", bars: (index === 0 || index === 3) && stock === 0 ? [bar(lastDay ? 810 : 900)] : [] }]));
    const retained = civil ? Object.fromEntries([0, 1].map((stock) => [stock, { entries: [history(stock, "2030-01-02", stock === 1 ? null : 900), ...(lastDay ? [history(stock, "2030-01-03", stock === 1 ? 1000 : 810)] : [])] }])) : {};
    return { update, view: { civilDate: dates[index], snapshot: { tick, day: days[index], accounts: clone(accounts), markets: clone(markets), daily_candles: { 0: Array(index >= 4 ? 362 : index >= 1 ? 361 : 360).fill(null), 1: Array(index >= 4 ? 361 : 360).fill(null) } }, runtimeSnapshot: { tick, day: days[index] }, workingOrders: [], currentMinute, retained, dayEndAccounts: clone(accounts), dayEndConfirmations: confirmations(lastDay) } };
  });
  return { closed: 1, open: 0, initial, frames };
}

describe("V11独立业务oracle的污染拒绝", { concurrency: 4 }, () => {
  it("接受显式构造的业务期望，不能冒真实native结果", () => assert.doesNotThrow(() => assertBusinessTrace(legalTrace(), "native-runtime")));
  const mutations = [
    ["首拍没有真实Trade", (trace) => { trace.frames[0].update.TickBatch.frames[0].facts = trace.frames[0].update.TickBatch.frames[0].facts.filter((fact) => !fact.event.Trade); }],
    ["closed伪造PriceTick", (trace) => { trace.frames[0].update.TickBatch.frames[0].facts.push({ event: { PriceTick: { code: 1 } } }); }],
    ["盘中未锁T+1", (trace) => { trace.frames[0].view.snapshot.accounts[0].positions[0].t1_locked = 0; }],
    ["错费但量额不变", (trace) => { trace.frames[2].view.dayEndConfirmations[0][0].actual_fees.commission = "499"; }],
    ["分钟时间错位", (trace) => { trace.frames[0].view.currentMinute[0].bars[0].minute_of_day = 571; }],
    ["分钟OHLC同总量污染", (trace) => { trace.frames[2].view.retained[0].entries[0].bars[0].high = "901"; }],
    ["closed日伪造零成交K", (trace) => { trace.frames[2].view.retained[1].entries[0].daily_candle = candle(1000, 0); }],
    ["复市漏发公开行情", (trace) => { delete trace.frames[3].update.TickBatch.frames[0].timeseries_payload.continuous_points[1]; }],
    ["native交割单错身份", (trace) => { trace.frames[2].view.dayEndConfirmations[0][0].receipt_id = "99"; }],
    ["Money非法科学计数法", (trace) => { trace.frames[2].view.dayEndAccounts[0].cash = "9e5"; }],
  ];
  for (const [name, mutate] of mutations) it(`拒绝${name}`, () => { const trace = legalTrace(); mutate(trace); assert.throws(() => assertBusinessTrace(trace, "native-runtime")); });
  it("拒绝serial跨账户复用全局receipt_id，即使各账户仍严格递增", () => {
    const trace = legalTrace();
    for (const view of trace.frames.map((entry) => entry.view)) {
      for (const leg of view.dayEndConfirmations[0]) leg.receipt_id = leg.civil_date === "2030-01-02" ? "1" : "2";
      for (const leg of view.dayEndConfirmations[1]) leg.receipt_id = leg.civil_date === "2030-01-02" ? "0" : "1";
    }
    assert.throws(() => assertBusinessTrace(trace, "wasm-serial"), /全局.*receipt_id/);
  });
  it("canonical仅排除已批准交割单receipt_id，其他差异保留", () => {
    const native = legalTrace(), serial = clone(native);
    for (const view of [serial.initial, ...serial.frames.map((entry) => entry.view)]) for (const rows of Object.values(view.dayEndConfirmations)) for (const leg of rows) leg.receipt_id = String(Number(leg.receipt_id) + 20);
    assertBusinessTrace(serial, "wasm-serial");
    assert.deepEqual(canonicalBusinessTrace(native), canonicalBusinessTrace(serial));
    serial.frames[5].view.dayEndAccounts[0].cash = "827997";
    assert.notDeepEqual(canonicalBusinessTrace(native), canonicalBusinessTrace(serial));
  });
});
