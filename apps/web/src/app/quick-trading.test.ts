import assert from "node:assert/strict";
import test from "node:test";
import { QuickTrading, fractionLots, pickBookDraft, buyingCapacity, applyFeeRate, tradingSecond } from "./quick-trading.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import type { Snapshot, Intent } from "../types/engine.ts";
function snapshot(tick = 0): Snapshot {
  return { tick, seq: tick, day: 0, phase: "Continuous", markets: {
    "600101": { last_price: "1000", last_close: "1000", cash_ex_reference_pending_trade: false, day_market_activity: false, last_cash_ex_reference: null, best_bid: "999", best_ask: "1000", bids: [["999", 100], ["998", 200]], asks: [["1000", 100], ["1001", 250]] },
    "002156": { last_price: "2000", last_close: "2000", cash_ex_reference_pending_trade: false, day_market_activity: false, last_cash_ex_reference: null, best_bid: "1999", best_ask: "2000", bids: [], asks: [] },
  }, accounts: { 0: { cash: "200000", reserved_cash: "0", positions: { "600101": { qty: 350, t1_locked: 100, invested_cents: "350000", recovered_cents: "0" } }, reserved_sell_qty: { "600101": 100 } } }, daily_candles: {}, active_daily_candles: {} };
}
function fixture() {
  let current = snapshot(); const submitted: Intent[] = [];
  const trading = new QuickTrading({ setup: () => DEFAULT_SETUP, snapshot: () => current, account: () => current.accounts["0"], autoAllowed: () => true, submit: async (intent: Intent) => { submitted.push(intent); } });
  return { trading, submitted, update: (next: Snapshot) => { current = next; return trading.advance(next); } };
}
test("游戏秒只累计存在的竞价阶段，自定义局跨日不会让自动提前到期", { timeout: 10000 }, async () => {
  for (const opening of [0, 900]) for (const closing of [0, 180]) {
    const setup = { ...DEFAULT_SETUP, ticks_per_day: 14400 + opening, auction_ticks: opening, closing_auction_ticks: closing };
    assert.equal(tradingSecond(setup.ticks_per_day, setup) - tradingSecond(setup.ticks_per_day - 1, setup), 1);
    assert.equal(tradingSecond(opening, setup), opening);
    let current = snapshot(setup.ticks_per_day - 1); const submitted: Intent[] = [];
    const trading = new QuickTrading({ setup: () => setup, snapshot: () => current, account: () => current.accounts["0"], autoAllowed: () => true, submit: async intent => { submitted.push(intent); } });
    trading.edit("600101", "Buy", { interval: 10 }); trading.toggleAuto("600101", "Buy");
    current = snapshot(setup.ticks_per_day); await trading.advance(current); assert.equal(submitted.length, 0);
    current = snapshot(setup.ticks_per_day + 9); await trading.advance(current); assert.equal(submitted.length, 1);
  }
});
test("比例舍尾保留零手，买入能力含费用与预留资金", { timeout: 10000 }, () => {
  assert.equal(fractionLots(1n, 2), 0n);
  assert.equal(fractionLots(17n, 3), 5n);
  assert.equal(buyingCapacity("100510", "1000", DEFAULT_SETUP.config), 1n);
  assert.equal(buyingCapacity("100499", "1000", DEFAULT_SETUP.config), 0n);
  assert.equal(applyFeeRate(1000n, 0.0005), 0n);
  assert.equal(applyFeeRate(3000n, 0.0005), 2n);
  assert.equal(applyFeeRate(9007199254740993n, 1), 9007199254740993n);
  assert.equal(buyingCapacity("1801448858147453341", "9007199254740993", { ...DEFAULT_SETUP.config, commission_rate: 1 }), 1n);
});
test("盘口价格点击不改数量，手数点击累加较优档并限制可用整手", { timeout: 10000 }, () => {
  const market = snapshot().markets["600101"]!;
  assert.deepEqual(pickBookDraft(market, "Buy", 1, "price", "7", 1n), { priceText: "10.01", quantityText: "7" });
  assert.deepEqual(pickBookDraft(market, "Buy", 1, "quantity", "7", 10n), { priceText: "10.01", quantityText: "3" });
  assert.deepEqual(pickBookDraft(market, "Sell", 1, "quantity", "7", 1n), { priceText: "9.98", quantityText: "1" });
});
test("买卖与各股票草稿独立，B1/S1跟随、手工修改退出跟随", { timeout: 10000 }, async () => {
  const f = fixture(); f.trading.edit("600101", "Buy", { quantityText: "2", priceMode: "bid" });
  assert.equal(f.trading.draft("600101", "Sell").quantityText, "1");
  f.trading.edit("002156", "Buy", { quantityText: "3" });
  assert.equal(f.trading.draft("600101", "Buy").quantityText, "2");
  assert.equal(f.trading.price("600101", "Buy"), "999");
  f.trading.edit("600101", "Sell", { priceMode: "ask" });
  const next = snapshot(1); next.markets["600101"]!.best_bid = "1002"; next.markets["600101"]!.best_ask = "1003";
  await f.update(next);
  assert.equal(f.trading.price("600101", "Buy"), "1002");
  assert.equal(f.trading.price("600101", "Sell"), "1003");
  next.markets["600101"]!.best_ask = null;
  assert.throws(() => f.trading.price("600101", "Sell"), /卖一暂无报价/);
  f.trading.edit("600101", "Buy", { priceText: "10.02", priceMode: "fixed" });
  assert.equal(f.trading.price("600101", "Buy"), "1002");
});
test("自动以游戏时间到期，不重复消费暂停进度，资源不足保持开启，仍绑定原股票", { timeout: 10000 }, async () => {
  const f = fixture(); f.trading.edit("600101", "Sell", { quantityText: "2" }); f.trading.toggleAuto("600101", "Sell");
  await f.update(snapshot(1)); assert.equal(f.submitted.length, 0);
  assert.equal(f.trading.jobs()[0]?.enabled, true); assert.match(f.trading.jobs()[0]!.status, /可卖/);
  const available = snapshot(2); available.accounts[0]!.reserved_sell_qty = {};
  await f.update(available); assert.equal(f.submitted.length, 1);
  await f.update(available); assert.equal(f.submitted.length, 1);
  f.trading.edit("002156", "Sell", { quantityText: "3" });
  assert.equal(f.trading.jobs()[0]?.code, "600101");
  assert.deepEqual(f.submitted[0], { PlaceLimit: { code: "600101", side: "Sell", price: { Fixed: "1000" }, qty: 200 } });
});
test("零手不提交，全部按可用整手，符号最大仍发Highest", { timeout: 10000 }, async () => {
  const f = fixture(); assert.equal(f.trading.capacity("600101", "Sell"), 1n);
  f.trading.edit("600101", "Buy", { quantityText: "0" }); assert.equal(await f.trading.submit("600101", "Buy"), false); assert.equal(f.submitted.length, 0);
  f.trading.edit("600101", "Buy", { quantityText: "1", priceMode: "highest" }); await f.trading.submit("600101", "Buy");
  assert.equal((f.submitted[0] as { PlaceLimit: { price: unknown } }).PlaceLimit.price, "Highest");
});
test("取消所有覆盖全股票双向且不停止自动", { timeout: 10000 }, async () => {
  const f = fixture(); f.trading.toggleAuto("600101", "Buy");
  await f.trading.cancelAll([{ code: "600101", id: 1 }, { code: "002156", id: 2 }]);
  assert.deepEqual(f.submitted, [{ Cancel: { code: "600101", id: 1 } }, { Cancel: { code: "002156", id: 2 } }]);
  assert.equal(f.trading.jobs()[0]?.enabled, true);
});
test("重新同步期间的批量撤单不会继续发往新会话", { timeout: 10000 }, async () => {
  const submitted: Intent[] = [];
  let release!: () => void;
  const trading = new QuickTrading({ setup: () => DEFAULT_SETUP, snapshot: () => snapshot(), account: () => snapshot().accounts["0"], autoAllowed: () => true, submit: async intent => {
    submitted.push(intent); if (submitted.length === 1) await new Promise<void>(resolve => { release = resolve; });
  } });
  const cancel = trading.cancelAll([{ code: "600101", id: 1 }, { code: "002156", id: 2 }]);
  trading.reset(); release();
  await assert.rejects(cancel, /会话.*变化/);
  assert.equal(submitted.length, 1);
});
test("撤单部分失败仍尝试同会话剩余订单并明确报告", { timeout: 10000 }, async () => {
  const ids: number[] = [];
  const trading = new QuickTrading({ setup: () => DEFAULT_SETUP, snapshot: () => snapshot(), account: () => snapshot().accounts["0"], autoAllowed: () => true, submit: async intent => {
    if (!("Cancel" in intent)) throw new Error("测试仅允许撤单");
    ids.push(intent.Cancel.id); if (intent.Cancel.id === 1) throw new Error("传输中断");
  } });
  await assert.rejects(trading.cancelAll([{ code: "600101", id: 1 }, { code: "002156", id: 2 }]), /#1：传输中断/);
  assert.deepEqual(ids, [1, 2]);
});
test("待处理买单占用跨证券可用资金，失败释放，不污染重新同步后状态", { timeout: 10000 }, async () => {
  let reject!: (error: Error) => void;
  const current = snapshot(); current.accounts[0]!.cash = "300000";
  const trading = new QuickTrading({ setup: () => DEFAULT_SETUP, snapshot: () => current, account: () => current.accounts["0"], autoAllowed: () => true, submit: () => new Promise<void>((_, fail) => { reject = fail; }) });
  assert.equal(trading.capacity("002156", "Buy"), 1n);
  const submitting = trading.submit("600101", "Buy");
  assert.equal(trading.busy("600101", "Buy"), true);
  assert.equal(trading.capacity("002156", "Buy"), 0n);
  assert.equal(await trading.submit("600101", "Buy"), false);
  trading.reset(); reject(new Error("旧会话断开")); await submitting;
  assert.equal(trading.busy("600101", "Buy"), false);
  assert.equal(trading.status("600101", "Buy"), "");
  assert.equal(trading.capacity("002156", "Buy"), 1n);
});
test("面板与跨股可买能力共用扣除pending的精确分现金，失败释放后恢复", { timeout: 10000 }, async () => {
  let reject!: (error: Error) => void;
  const current = snapshot(); current.accounts[0]!.cash = "9007199254740993"; current.accounts[0]!.reserved_cash = "123";
  const trading = new QuickTrading({ setup: () => DEFAULT_SETUP, snapshot: () => current, account: () => current.accounts["0"], autoAllowed: () => true, submit: () => new Promise<void>((_, fail) => { reject = fail; }) });
  assert.equal(trading.availableCash(), "9007199254740870");
  const submitting = trading.submit("600101", "Buy");
  assert.equal(trading.availableCash(), "9007199254640369");
  assert.equal(trading.capacity("002156", "Buy"), buyingCapacity(trading.availableCash(), "2000", DEFAULT_SETUP.config));
  reject(new Error("提交失败")); assert.equal(await submitting, false);
  assert.equal(trading.availableCash(), "9007199254740870");
});
test("合法不足一手零股仍可整笔卖出，买入小数手拒绝", { timeout: 10000 }, async () => {
  const f = fixture(), next = snapshot(1); next.accounts[0]!.positions["600101"]!.qty = 250;
  await f.update(next); f.trading.edit("600101", "Sell", { quantityText: "0.50" });
  assert.equal(await f.trading.submit("600101", "Sell"), true);
  assert.deepEqual(f.submitted[0], { PlaceLimit: { code: "600101", side: "Sell", price: { Fixed: "1000" }, qty: 50 } });
  f.trading.edit("600101", "Buy", { quantityText: "0.5" }); assert.equal(await f.trading.submit("600101", "Buy"), false);
});
test("暂停确认发生在提交等待期间，剩余到期自动任务不会下单", { timeout: 10000 }, async () => {
  let running = true, current = snapshot(); let release!: () => void;
  const submitted: Intent[] = [];
  const trading = new QuickTrading({ setup: () => DEFAULT_SETUP, snapshot: () => current, account: () => current.accounts["0"], autoAllowed: () => running, submit: async intent => {
    submitted.push(intent); if (submitted.length === 1) await new Promise<void>(resolve => { release = resolve; });
  } });
  trading.toggleAuto("600101", "Buy"); trading.toggleAuto("002156", "Buy");
  current = snapshot(1); current.accounts[0]!.cash = "1000000";
  const advancing = trading.advance(current); running = false; release(); await advancing;
  assert.equal(submitted.length, 1);
  assert.equal(trading.jobs().every(job => job.enabled), true);
});
test("共享市场快捷委托仅使用本人非零账户，缺席不能借用账户0余额", { timeout: 10000 }, async () => {
  const current = snapshot();
  current.accounts["7"] = { ...current.accounts["0"], cash: "100510", positions: {}, reserved_sell_qty: {} };
  current.accounts["0"].cash = "0";
  let own: import("../types/engine.ts").AccountSnap | null = current.accounts["7"];
  const submitted: Intent[] = [];
  const trading = new QuickTrading({ setup: () => DEFAULT_SETUP, snapshot: () => current, account: () => own, autoAllowed: () => true, submit: async (intent) => { submitted.push(intent); } });
  assert.equal(trading.capacity("600101", "Buy"), 1n);
  assert.equal(trading.availableCash(), "100510");
  own = null;
  assert.throws(() => trading.availableCash(), /本人.*账户/);
  assert.equal(await trading.submit("600101", "Buy"), false);
  assert.equal(submitted.length, 0);
});
