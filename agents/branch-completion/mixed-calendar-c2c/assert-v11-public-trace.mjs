// C2-C 零NPC代表性fixture的独立业务oracle，不负责native/wasm相互对照。
import assert from "node:assert/strict";

export function assertMixedPublicTrace(trace) {
  const marketKinds = new Set(["PriceTick", "Trade", "AuctionTick", "AuctionCompleted"]);
  const boundaries = [];
  for (const [index, entry] of trace.frames.slice(0, 2).entries()) {
    assert(entry.update.TickBatch, "Jan2市场拍必须为真实TickBatch");
    let openPrices = 0;
    for (const frame of entry.update.TickBatch.frames) {
      const payload = frame.timeseries_payload;
      for (const field of ["active_daily_candles", "continuous_points", "auction_points", "closed_daily_candles"]) {
        assert(payload[field] && typeof payload[field] === "object", `公开timeseries缺少${field}`);
        assert(!Object.hasOwn(payload[field], trace.closed), `closed股不得出现在公开timeseries ${field}`);
      }
      assert(Object.hasOwn(payload.continuous_points, trace.open), "open股必须有连续行情timeseries正控");
      if (index === 1) assert(Object.hasOwn(payload.closed_daily_candles, trace.open), "open股必须有终K timeseries正控");
      for (const fact of frame.facts) {
        const kinds = Object.keys(fact.event);
        assert.equal(kinds.length, 1, "公开event必须只有一个variant");
        const kind = kinds[0];
        const event = fact.event[kind];
        if (marketKinds.has(kind)) assert.notEqual(event.code, trace.closed, `closed股不得发布${kind}`);
        if (kind === "PriceTick" && event.code === trace.open) openPrices += 1;
        if (kind === "DayBoundary") boundaries.push(event);
      }
    }
    assert(openPrices > 0, "每个Jan2市场拍都必须有open PriceTick正控");
  }
  assert.equal(boundaries.length, 1, "Jan2必须恰有一个真实DayBoundary");
  assert.deepEqual(Object.keys(boundaries[0].closed_daily_candles), [String(trace.open)], "DayBoundary只可交付open股终K");
  const first = trace.frames[0].view;
  assert.equal(first.currentMinute[trace.closed].trading, false);
  assert.equal(first.currentMinute[trace.closed].phase, "Closed");
  assert(!Object.hasOwn(first.snapshot.active_daily_candles, trace.closed));
  assert.equal(trace.frames[1].view.snapshot.daily_candles[trace.closed].length, 360);
  assert.equal(trace.frames[1].view.snapshot.daily_candles[trace.open].length, 361);
  assert.equal(trace.frames[4].view.snapshot.daily_candles[trace.closed].length, 361);
  assert.equal(trace.frames[4].view.snapshot.daily_candles[trace.open].length, 362);
  assert.equal(trace.frames[2].view.retained[trace.closed].entries[0].availability, "Closed");
  assert.equal(trace.frames[2].view.retained[trace.open].entries[0].availability, "NoTrades");
}
