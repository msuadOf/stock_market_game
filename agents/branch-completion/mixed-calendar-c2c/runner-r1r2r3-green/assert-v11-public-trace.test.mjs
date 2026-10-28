import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { assertMixedPublicTrace } from "./assert-v11-public-trace.mjs";

function legalTrace() {
  const view = () => ({
    currentMinute: { 1: { trading: false, phase: "Closed" } },
    snapshot: { active_daily_candles: { 0: {} }, daily_candles: { 0: Array(361), 1: Array(360) } },
    retained: { 0: { entries: [{ availability: "NoTrades" }] }, 1: { entries: [{ availability: "Closed" }] } },
  });
  const trace = { closed: 1, open: 0, frames: Array.from({ length: 6 }, () => ({
    update: { TickBatch: { frames: [{ facts: [{ event: { PriceTick: { code: 0 } } }] }] } },
    view: view(),
  })) };
  trace.frames[1].update.TickBatch.frames[0].facts.push({ event: { DayBoundary: { closed_daily_candles: { 0: {} } } } });
  trace.frames[4].view.snapshot.daily_candles = { 0: Array(362), 1: Array(361) };
  return trace;
}

describe("V11独立公开市场事实oracle", { concurrency: 8 }, () => {
  it("接受已有snapshot与真实open行情及仅open终K", () => {
    assert.doesNotThrow(() => assertMixedPublicTrace(legalTrace()));
  });

  for (const kind of ["PriceTick", "Trade", "AuctionTick", "AuctionCompleted"]) {
    it(`R3仅污染公开${kind}为closed也必须拒绝`, () => {
      const trace = legalTrace();
      trace.frames[0].update.TickBatch.frames[0].facts.push({ event: { [kind]: { code: 1 } } });
      assert.throws(() => assertMixedPublicTrace(trace), /closed/);
    });
  }

  it("R3仅污染DayBoundary把closed终K加入也必须拒绝", () => {
    const trace = legalTrace();
    trace.frames[1].update.TickBatch.frames[0].facts[1].event.DayBoundary.closed_daily_candles[1] = {};
    assert.throws(() => assertMixedPublicTrace(trace), /DayBoundary/);
  });

  it("R3每个Jan2真实市场拍都必须有open行情正控", () => {
    const trace = legalTrace();
    trace.frames[0].update.TickBatch.frames[0].facts = [];
    assert.throws(() => assertMixedPublicTrace(trace), /open/);
  });
});
