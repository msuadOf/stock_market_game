// C2-C 零NPC代表性fixture的独立业务oracle，不负责native/wasm相互对照。
import assert from "node:assert/strict";

export function assertMixedPublicTrace(trace) {
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
