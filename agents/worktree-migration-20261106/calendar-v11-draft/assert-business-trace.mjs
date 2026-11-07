// V11 tests-first 验收源码：独立业务oracle；不能用native/WASM两份相同错误自证。
import assert from "node:assert/strict";

const cents = (value) => {
  assert.equal(typeof value, "string", "Money、费用与u64_decimal必须保十进制字符串");
  assert.match(value, /^(?:0|[1-9][0-9]*)$/);
  return BigInt(value);
};
const events = (entry) => entry.update.TickBatch.frames.flatMap((frame) => frame.facts.map((fact) => fact.event));
const qty = (account, stock) => account.positions[stock]?.qty ?? 0;
const sum = (rows, select) => rows.reduce((total, row) => total + select(row), 0n);
function bar(value, price, volume = 100n) {
  assert.equal(value.minute_of_day, 570, "固定TPD2首拍成交属于真实09:30桶");
  assert.equal(value.phase, "Continuous");
  for (const key of ["open", "high", "low", "close"]) assert.equal(cents(value[key]), price);
  assert.equal(cents(value.volume_shares), volume);
  assert.equal(cents(value.turnover_cents), price * volume);
  assert.equal(cents(value.trade_count), 1n);
}
function candle(value, price, volume = 100n) {
  for (const key of ["open", "high", "low", "close"]) assert.equal(cents(value[key]), price);
  assert.equal(BigInt(value.volume), volume);
  assert.equal(cents(value.trade_stats.turnover_cents), price * volume);
  assert.equal(BigInt(value.trade_stats.trade_count), volume === 0n ? 0n : 1n);
}
function current(value, stock, date, phase, trading) {
  assert.equal(value.stock, stock);assert.equal(value.date, date);assert.equal(value.live, true);
  assert.equal(value.trading, trading);assert.equal(value.phase, phase);
}
function legs(view, stock, date) {
  return Object.fromEntries(["0", "1"].map((owner) => [owner,
    view.dayEndConfirmations[owner].filter((leg) => leg.stock === stock && leg.civil_date === date)]));
}
function confirmation(leg, side, stock, date, price, tax) {
  assert.equal(leg.side, side);assert.equal(leg.stock, stock);assert.equal(leg.civil_date, date);
  assert.equal(leg.quantity_shares, 100);assert.equal(cents(leg.price_cents), price);
  assert.equal(cents(leg.gross_cents), price * 100n);
  assert.equal(cents(leg.actual_fees.commission), 500n);
  assert.equal(cents(leg.actual_fees.transfer_fee), 1n);
  assert.equal(cents(leg.actual_fees.stamp_tax), tax);
  cents(leg.receipt_id);
}

export function assertBusinessTrace(trace, pipeline) {
  assert(["native-runtime", "wasm-serial"].includes(pipeline), "必须由真实runner声明管线来源");
  assert.equal(trace.frames.length, 6);assert.notEqual(trace.closed, trace.open);
  assert.equal(trace.initial.civilDate, "2030-01-02");
  assert.equal(trace.initial.snapshot.tick, 0);assert.equal(trace.initial.snapshot.day, 0);
  assert.deepEqual(Object.keys(trace.initial.dayEndAccounts).sort(), ["0", "1"]);
  assert.deepEqual(trace.initial.workingOrders, []);
  assert.equal(qty(trace.initial.dayEndAccounts["0"], trace.open), 0);
  assert.equal(qty(trace.initial.dayEndAccounts["1"], trace.open), 1000);
  const expectedTicks = [1, 2, 2, 3, 4, 4];
  const expectedDays = [0, 1, 1, 1, 2, 2];
  const expectedDates = ["2030-01-02", "2030-01-02", "2030-01-03", "2030-01-03", "2030-01-03", "2030-01-04"];
  for (const [index, entry] of trace.frames.entries()) {
    const kind = index % 3 === 2 ? "CivilUpdate" : "TickBatch";
    assert.deepEqual(Object.keys(entry.update), [kind]);
    assert.equal(entry.view.civilDate, expectedDates[index]);
    assert.equal(entry.view.snapshot.tick, expectedTicks[index]);
    assert.equal(entry.view.runtimeSnapshot.tick, expectedTicks[index]);
    assert.equal(entry.view.snapshot.day, expectedDays[index]);
    assert.equal(entry.view.runtimeSnapshot.day, expectedDays[index]);
    if (kind === "TickBatch") {
      assert.equal(entry.update.TickBatch.frames.length, 1);
      const frame = entry.update.TickBatch.frames[0];
      assert.equal(frame.tick, expectedTicks[index]);
      const openSet = (index < 2 ? [trace.open] : [trace.closed, trace.open]).map(String).sort();
      assert.deepEqual(Object.keys(frame.timeseries_payload.continuous_points).sort(), openSet,
        "公开continuous_points必须逐帧交付真实开市集合");
      const frameEvents = events(entry);
      assert.deepEqual(frameEvents.flatMap((event) => event.PriceTick ? [String(event.PriceTick.code)] : []).sort(), openSet,
        "每个开市股必须有真实PriceTick，复市不得只更新私有分钟镜像");
      const boundaries = frameEvents.flatMap((event) => event.DayBoundary ? [event.DayBoundary] : []);
      if (index % 3 === 1) {
        assert.equal(boundaries.length, 1);
        assert.deepEqual(Object.keys(boundaries[0].closed_daily_candles).sort(), openSet);
        assert.deepEqual(Object.keys(frame.timeseries_payload.closed_daily_candles).sort(), openSet);
      } else {
        assert.deepEqual(boundaries, []);assert.deepEqual(frame.timeseries_payload.closed_daily_candles, {});
      }
    } else {
      assert.equal(entry.update.CivilUpdate.tick, expectedTicks[index]);
      assert.equal(entry.update.CivilUpdate.civil_date, expectedDates[index]);
    }
    assert.deepEqual(entry.view.workingOrders, [], "唯一玩家100股请求须当拍全成，不能残留活动订单");
  }
  const firstTrades = events(trace.frames[0]).flatMap((event) => event.Trade ? [event.Trade] : []);
  assert.equal(firstTrades.length, 1, "首拍必须真实Panic对玩家成交，不延后到日结");
  assert.deepEqual(events(trace.frames[1]).filter((event) => event.Trade), []);
  const sellRejections = events(trace.frames[1]).flatMap((event) => event.IntentRejected ? [event.IntentRejected] : []);
  assert.equal(sellRejections.length, 1, "真实买入后的当日卖单须恰有一次拒绝");
  assert.equal(sellRejections[0].account, "0");
  assert.equal(sellRejections[0].code, trace.open);
  assert.equal(sellRejections[0].reason, "InsufficientShares");
  let rejected = 0;
  for (const entry of trace.frames.slice(0, 2)) {
    current(entry.view.currentMinute[trace.closed], trace.closed, "2030-01-02", "Closed", false);
    assert.deepEqual(entry.view.currentMinute[trace.closed].bars, []);
    assert.deepEqual(entry.view.snapshot.markets[trace.closed], trace.initial.snapshot.markets[trace.closed]);
    for (const frame of entry.update.TickBatch.frames) {
      for (const field of ["active_daily_candles", "continuous_points", "auction_points", "closed_daily_candles"]) {
        assert(!Object.hasOwn(frame.timeseries_payload[field], trace.closed), `closed不得产生${field}`);
      }
    }
    for (const event of events(entry)) {
      assert.equal(Object.keys(event).length, 1);
      for (const kind of ["Trade", "PriceTick", "AuctionTick", "AuctionCompleted"]) {
        if (event[kind]) assert.notEqual(event[kind].code, trace.closed);
      }
      if (event.IntentRejected?.code === trace.closed) {
        assert.equal(event.IntentRejected.reason, "ExchangeClosed");rejected++;
      }
    }
  }
  assert.equal(rejected, 1);
  const first = firstTrades[0];
  assert.equal(first.code, trace.open);assert.equal(first.maker, "1");assert.equal(first.taker, "0");
  assert.equal(first.qty, 100);assert.equal(cents(first.price), 900n);
  const intraday = trace.frames[0].view;
  assert.equal(qty(intraday.snapshot.accounts["0"], trace.open), 100);
  assert.equal(intraday.snapshot.accounts["0"].positions[trace.open].t1_locked, 100, "首拍真实买入须锁T+1");
  assert(!Object.hasOwn(intraday.snapshot.accounts["0"].positions, trace.closed));
  current(intraday.currentMinute[trace.open], trace.open, "2030-01-02", "Continuous", true);
  assert.equal(intraday.currentMinute[trace.open].bars.length, 1);bar(intraday.currentMinute[trace.open].bars[0], 900n);
  const endMarket = trace.frames[1].view;
  assert.equal(endMarket.snapshot.daily_candles[trace.closed].length, 360);
  assert.equal(endMarket.snapshot.daily_candles[trace.open].length, 361);
  current(endMarket.currentMinute[trace.open], trace.open, "2030-01-02", "AfterClose", true);
  assert.deepEqual(endMarket.currentMinute[trace.open].bars, [], "当日分钟桶已迁入retained");
  const buyerEnd = endMarket.snapshot.accounts["0"];
  assert.equal(buyerEnd.positions[trace.open].t1_locked, 0, "成功市场日结解锁open T+1");
  assert.equal(cents(buyerEnd.reserved_cash), 0n);assert.deepEqual(buyerEnd.reserved_sell_qty, {});
  const boundary = trace.frames[2].view;
  const openHistory = boundary.retained[trace.open].entries;
  const closedHistory = boundary.retained[trace.closed].entries;
  assert.equal(openHistory.length, 1);assert.equal(closedHistory.length, 1);
  assert.equal(closedHistory[0].date, "2030-01-02");assert.equal(closedHistory[0].availability, "Closed");
  assert.equal(closedHistory[0].daily_candle, null);assert.deepEqual(closedHistory[0].bars, []);
  assert.equal(openHistory[0].date, "2030-01-02");assert.equal(openHistory[0].availability, "Traded");
  candle(openHistory[0].daily_candle, 900n);
  assert.equal(openHistory[0].bars.length, 1);bar(openHistory[0].bars[0], 900n);
  const pair = legs(boundary, trace.open, "2030-01-02");
  assert.equal(pair["0"].length, 1);assert.equal(pair["1"].length, 1);
  const buyer = pair["0"][0], seller = pair["1"][0];
  confirmation(buyer, "Buy", trace.open, "2030-01-02", 900n, 0n);
  confirmation(seller, "Sell", trace.open, "2030-01-02", 900n, 45n);
  assert.notEqual(buyer.receipt_id, seller.receipt_id);
  if (pipeline === "native-runtime") {
    assert.equal(cents(seller.receipt_id), BigInt(first.seq) * 2n);
    assert.equal(cents(buyer.receipt_id), BigInt(first.seq) * 2n + 1n);
  }
  const accounts = boundary.dayEndAccounts;
  assert.equal(cents(trace.initial.dayEndAccounts["0"].cash) - cents(accounts["0"].cash), 90501n);
  assert.equal(cents(accounts["1"].cash) - cents(trace.initial.dayEndAccounts["1"].cash), 89454n);
  assert.equal(qty(accounts["1"], trace.open), 900);
  assert.equal(qty(accounts["0"], trace.open) + qty(accounts["1"], trace.open), 1000);
  assert.equal(qty(accounts["1"], trace.closed), 1000);
  // Jan3两股复市：公开面只暴露墙钟minute_of_day，不冒内部MarketMinuteClose absolute键。
  const resumed = trace.frames[3].view;
  const resumedTrades = events(trace.frames[3]).flatMap((event) => event.Trade ? [event.Trade] : []);
  assert.equal(resumedTrades.length, 1);
  const resumedTrade = resumedTrades[0];
  assert.equal(resumedTrade.code, trace.open);assert.equal(resumedTrade.qty, 100);
  assert.equal(cents(resumedTrade.price), 810n);assert.equal(resumedTrade.maker, "1");assert.equal(resumedTrade.taker, "0");
  for (const stock of [trace.closed, trace.open]) {
    current(resumed.currentMinute[stock], stock, "2030-01-03", "Continuous", true);
    current(trace.frames[4].view.currentMinute[stock], stock, "2030-01-03", "AfterClose", true);
    const history = trace.frames[5].view.retained[stock].entries;
    assert.equal(history.length, 2);assert.equal(history[1].date, "2030-01-03");
    if (stock === trace.open) {
      assert.equal(resumed.currentMinute[stock].bars.length, 1);bar(resumed.currentMinute[stock].bars[0], 810n);
      assert.equal(resumed.snapshot.accounts["0"].positions[stock].t1_locked, 100);
      assert.equal(trace.frames[4].view.snapshot.accounts["0"].positions[stock].t1_locked, 0);
      assert.equal(history[1].availability, "Traded");candle(history[1].daily_candle, 810n);
      assert.equal(history[1].bars.length, 1);bar(history[1].bars[0], 810n);
    } else {
      // 单NPC一次只选一股；复市closed只验真实开市零成交，不假造第二笔对手成交。
      assert.deepEqual(resumed.currentMinute[stock].bars, []);
      assert(!Object.hasOwn(resumed.snapshot.accounts["0"].positions, stock));
      assert.equal(history[1].availability, "NoTrades");assert.deepEqual(history[1].bars, []);
      const mark = history[1].daily_candle;
      assert(mark, "开市零成交日须有真实mark日K，不能仍按Closed处理");
      for (const key of ["open", "high", "low", "close"]) assert.equal(cents(mark[key]), 1000n);
      assert.equal(BigInt(mark.volume), 0n);assert.equal(cents(mark.trade_stats.turnover_cents), 0n);
      assert.equal(BigInt(mark.trade_stats.trade_count), 0n);
    }
  }
  const finalView = trace.frames[5].view;
  const resumedPair = legs(finalView, trace.open, "2030-01-03");
  assert.equal(resumedPair["0"].length, 1);assert.equal(resumedPair["1"].length, 1);
  const resumedBuyer = resumedPair["0"][0], resumedSeller = resumedPair["1"][0];
  confirmation(resumedBuyer, "Buy", trace.open, "2030-01-03", 810n, 0n);
  confirmation(resumedSeller, "Sell", trace.open, "2030-01-03", 810n, 40n);
  assert.notEqual(resumedBuyer.receipt_id, resumedSeller.receipt_id);
  if (pipeline === "native-runtime") {
    assert.equal(cents(resumedSeller.receipt_id), BigInt(resumedTrade.seq) * 2n);
    assert.equal(cents(resumedBuyer.receipt_id), BigInt(resumedTrade.seq) * 2n + 1n);
  }
  assert.equal(cents(accounts["0"].cash) - cents(finalView.dayEndAccounts["0"].cash), 81501n);
  assert.equal(cents(finalView.dayEndAccounts["1"].cash) - cents(accounts["1"].cash), 80459n);
  assert.equal(qty(finalView.dayEndAccounts["0"], trace.open), 200);
  assert.equal(qty(finalView.dayEndAccounts["1"], trace.open), 800);
  assert.equal(qty(finalView.dayEndAccounts["1"], trace.closed), 1000);
  for (const owner of ["0", "1"]) assert.equal(finalView.dayEndConfirmations[owner].length, 2);
  const allReceiptIds = Object.values(finalView.dayEndConfirmations).flatMap((rows) => rows.map((leg) => cents(leg.receipt_id)));
  assert.equal(new Set(allReceiptIds).size, allReceiptIds.length, "全局receipt_id必须唯一，不能跨账户复用不同成交腿身份");
  assert.deepEqual(events(trace.frames[4]).filter((event) => event.Trade), []);
  assert.equal(trace.frames[4].view.snapshot.daily_candles[trace.closed].length, 361);
  assert.equal(trace.frames[4].view.snapshot.daily_candles[trace.open].length, 362);
  for (const owner of ["0", "1"]) {
    const ids = trace.frames[5].view.dayEndConfirmations[owner].map((leg) => cents(leg.receipt_id));
    assert(ids.every((id, index) => index === 0 || id > ids[index - 1]), "各自管线公开id须严格单调且唯一");
  }
}

export function canonicalBusinessTrace(trace) {
  const result = structuredClone(trace);
  // 只有已登记的不同id空间允许排除；raw trace必须先另存，其他差异仍使对拍失败。
  for (const view of [result.initial, ...result.frames.map((entry) => entry.view)]) {
    for (const rows of Object.values(view.dayEndConfirmations)) for (const leg of rows) delete leg.receipt_id;
  }
  return result;
}

// 两端分别满足业务约束后才比较，避免一致但错误的输出自证。
export function assertBusinessParity(nativeTrace, browserResult) {
  assert.equal(browserResult?.ok, true, "真实浏览器 driver 未成功");
  assert.equal(browserResult.result?.mode, "real-browser-wasm32-serial", "浏览器执行模式不符");
  assertBusinessTrace(nativeTrace, "native-runtime");
  assertBusinessTrace(browserResult.result.trace, "wasm-serial");
  assert.deepEqual(canonicalBusinessTrace(nativeTrace), canonicalBusinessTrace(browserResult.result.trace), "跨宿主业务 trace 不一致");
}

function hostUpdate(frame) {
  const updates = frame.updates.filter((item) => item.type === "protocol");
  assert.equal(updates.length, 1, "每步须恰有一个真实 protocol 更新");
  return updates[0].update;
}

// 产品宿主只公开玩家账户；此 oracle 不构造 NPC 账户或其交割单。
export function assertHostBusinessTrace(trace) {
  assert.equal(trace.ok, true);
  assert.equal(trace.initial.civilDate, "2030-01-02");
  assert.equal(trace.frames.length, 6);
  const [first, end, boundary, resumed, reopenedEnd, final] = trace.frames;
  const facts = (frame) => events({ update: hostUpdate(frame) });
  const firstTrades = facts(first).flatMap(event => event.Trade ? [event.Trade] : []);
  assert.equal(firstTrades.length, 1);
  assert.equal(firstTrades[0].code, 0);assert.equal(firstTrades[0].qty, 100);assert.equal(cents(firstTrades[0].price), 900n);
  const closedReject = facts(first).flatMap(event => event.IntentRejected ? [event.IntentRejected] : []);
  assert.equal(closedReject.length, 1);assert.equal(closedReject[0].code, 1);assert.equal(closedReject[0].reason, "ExchangeClosed");
  const sellReject = facts(end).flatMap(event => event.IntentRejected ? [event.IntentRejected] : []);
  assert.equal(sellReject.length, 1);assert.equal(sellReject[0].account, "0");assert.equal(sellReject[0].code, 0);assert.equal(sellReject[0].reason, "InsufficientShares");
  assert.equal(first.view.snapshot.accounts["0"].positions["000101"].t1_locked, 100);
  assert.equal(cents(trace.initial.snapshot.accounts["0"].cash) - cents(first.view.snapshot.accounts["0"].cash), 90501n);
  assert.equal(end.view.snapshot.accounts["0"].positions["000101"].t1_locked, 0);
  assert.equal(first.view.currentMinute["600101"].trading, false);assert.equal(first.view.currentMinute["600101"].phase, "Closed");assert.deepEqual(first.view.currentMinute["600101"].bars, []);
  assert.equal(first.view.currentMinute["000101"].bars.length, 1);bar(first.view.currentMinute["000101"].bars[0], 900n);
  const closed = boundary.view.retained["600101"].entries[0], open = boundary.view.retained["000101"].entries[0];
  assert.equal(closed.date, "2030-01-02");assert.equal(closed.availability, "Closed");assert.equal(closed.daily_candle, null);assert.deepEqual(closed.bars, []);
  assert.equal(open.availability, "Traded");candle(open.daily_candle, 900n);bar(open.bars[0], 900n);
  assert.equal(boundary.view.confirmations.length, 1);
  const firstReceipt = boundary.view.confirmations[0];
  assert.equal(firstReceipt.code, "000101");assert.equal(firstReceipt.side, "Buy");assert.equal(firstReceipt.civil_date, "2030-01-02");assert.equal(firstReceipt.quantity_shares, 100);assert.equal(cents(firstReceipt.price), 900n);assert.equal(cents(firstReceipt.gross), 90000n);
  assert.equal(cents(firstReceipt.actual_fees.commission), 500n);assert.equal(cents(firstReceipt.actual_fees.transfer_fee), 1n);assert.equal(cents(firstReceipt.actual_fees.stamp_tax), 0n);
  const trades = facts(resumed).flatMap(event => event.Trade ? [event.Trade] : []);
  assert.equal(trades.length, 1);assert.equal(trades[0].code, 0);assert.equal(trades[0].qty, 100);assert.equal(cents(trades[0].price), 810n);
  assert.equal(resumed.view.currentMinute["600101"].trading, true);assert.equal(resumed.view.currentMinute["600101"].phase, "Continuous");
  assert.equal(resumed.view.snapshot.accounts["0"].positions["000101"].t1_locked, 100);
  assert.equal(reopenedEnd.view.snapshot.accounts["0"].positions["000101"].t1_locked, 0);
  assert.equal(cents(trace.initial.snapshot.accounts["0"].cash) - cents(final.view.snapshot.accounts["0"].cash), 172002n);
  const reopened = final.view.retained["600101"].entries[1], traded = final.view.retained["000101"].entries[1];
  assert.equal(reopened.date, "2030-01-03");assert.equal(reopened.availability, "NoTrades");assert.deepEqual(reopened.bars, []);candle(reopened.daily_candle, 1000n, 0n);
  assert.equal(traded.availability, "Traded");candle(traded.daily_candle, 810n);bar(traded.bars[0], 810n);
  assert.equal(final.view.confirmations.length, 2);
  const resumedReceipt = final.view.confirmations[0];
  assert.equal(resumedReceipt.code, "000101");assert.equal(resumedReceipt.side, "Buy");assert.equal(resumedReceipt.civil_date, "2030-01-03");assert.equal(resumedReceipt.quantity_shares, 100);assert.equal(cents(resumedReceipt.price), 810n);assert.equal(cents(resumedReceipt.gross), 81000n);
  assert.equal(cents(resumedReceipt.actual_fees.commission), 500n);assert.equal(cents(resumedReceipt.actual_fees.transfer_fee), 1n);assert.equal(cents(resumedReceipt.actual_fees.stamp_tax), 0n);
}

export function assertHostBusinessParity(native, host) {
  assertBusinessTrace(native, "native-runtime");
  assertHostBusinessTrace(host);
  for (const [index, frame] of host.frames.entries()) {
    const nativeUpdate = native.frames[index].update, browserUpdate = hostUpdate(frame);
    if (nativeUpdate.TickBatch) assert.deepEqual(browserUpdate.TickBatch.frames, nativeUpdate.TickBatch.frames, `第 ${index} 步公开市场帧不一致`);
    else assert.deepEqual(browserUpdate, nativeUpdate, `第 ${index} 步自然日更新不一致`);
    const expectedView = native.frames[index].view;
    assert.equal(frame.view.civilDate, expectedView.civilDate);
    const codeOf = (id) => {
      if (String(id) === String(native.open)) return "000101";
      if (String(id) === String(native.closed)) return "600101";
      throw new Error(`未知 StockId：${id}`);
    };
    const mapStocks = (map) => Object.fromEntries(Object.entries(map).map(([id, value]) => [codeOf(id), value]));
    const expectedSnapshot = structuredClone(expectedView.snapshot);
    for (const field of ["markets", "daily_candles", "active_daily_candles"]) if (expectedSnapshot[field] !== undefined) expectedSnapshot[field] = mapStocks(expectedSnapshot[field]);
    const player = expectedSnapshot.accounts["0"];
    player.positions = mapStocks(player.positions);
    player.reserved_sell_qty = mapStocks(player.reserved_sell_qty);
    expectedSnapshot.accounts = { "0": player };
    assert.deepEqual(frame.view.snapshot, expectedSnapshot, `第 ${index} 步玩家公开快照不一致`);
    assert.deepEqual(frame.view.workingOrders, expectedView.workingOrders, `第 ${index} 步玩家活动委托不一致`);
    if (index === 2 || index === 5) {
      const confirmations = expectedView.dayEndConfirmations["0"].map(({ stock, price_cents, gross_cents, ...leg }) => ({ ...leg, code: codeOf(stock), price: price_cents, gross: gross_cents })).reverse();
      assert.deepEqual(frame.view.confirmations, confirmations, `第 ${index} 步玩家交割单不一致`);
    }
    for (const id of [native.open, native.closed]) {
      const code = codeOf(id);
      const minute = structuredClone(expectedView.currentMinute[id]);delete minute.stock;minute.code = code;
      assert.deepEqual(frame.view.currentMinute[code], minute, `第 ${index} 步分钟查询不一致`);
      if (Object.hasOwn(expectedView.retained, id)) {
        const history = structuredClone(expectedView.retained[id]);delete history.stock;history.code = code;
        assert.deepEqual(frame.view.retained[code], history, `第 ${index} 步保留历史不一致`);
      } else assert(!Object.hasOwn(frame.view.retained, code));
    }
  }
}
