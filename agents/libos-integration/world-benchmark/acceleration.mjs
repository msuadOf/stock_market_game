import assert from 'node:assert/strict';

// 只评估一个交易日内、连续交易阶段的窗口；跨阶段须另行建模。
export function continuousAcceleration(setup, window, elapsedMs) {
  const day = setup.ticks_per_day, opening = setup.auction_ticks, closing = setup.closing_auction_ticks;
  for (const n of [day, opening, closing, window.from, window.to]) assert.ok(Number.isSafeInteger(n) && n >= 0, '时钟输入必须是非负安全整数');
  assert.ok(day > opening + closing, '连续交易 tick 必须大于零');
  assert.ok(window.from >= opening && window.to <= day - closing && window.to > window.from, '窗口必须完整落在单日连续交易阶段');
  assert.ok(Number.isFinite(elapsedMs) && elapsedMs > 0, '实际耗时必须大于零');
  const ticks = BigInt(day - opening - closing);
  const seconds = BigInt(closing > 0 ? 14220 : 14400);
  const offset = tick => BigInt(tick - opening) * seconds / ticks;
  const simulatedSeconds = Number(offset(window.to) - offset(window.from));
  assert.ok(simulatedSeconds > 0, '窗口的模拟时间不足一秒');
  const acceleration = simulatedSeconds * 1000 / elapsedMs;
  assert.ok(Number.isFinite(acceleration), '加速比超出有限数范围');
  return {simulatedSeconds, acceleration};
}
