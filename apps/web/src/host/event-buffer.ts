import type { EngineEvent } from "../types/engine";

/** UI 背压只能阻止发消息，不能阻止引擎继续推进。 */
export function uiBackpressurePolicy(awaitingUiFrame: boolean): {
  stepEngine: true;
  flushUi: boolean;
} {
  return { stepEngine: true, flushUi: !awaitingUiFrame };
}

/** serde-wasm-bindgen 会把 Rust BTreeMap 变成 Map；只在稀有日界做窄转换。 */
export function normalizeEventMaps(events: EngineEvent[]): EngineEvent[] {
  let normalized: EngineEvent[] | null = null;
  events.forEach((event, index) => {
    if (!("DayBoundary" in event)) return;
    const candles = event.DayBoundary.closed_daily_candles;
    if (!(candles instanceof Map)) return;
    normalized ??= [...events];
    normalized[index] = {
      DayBoundary: {
        ...event.DayBoundary,
        closed_daily_candles: Object.fromEntries(candles),
      },
    };
  });
  return normalized ?? events;
}

/**
 * “最快”只描述引擎推进速度，不代表 React 必须渲染每个中间 tick。
 *
 * 跨日事件携带完整的权威收盘 K 线，必须全部保留；最后一个交易日内，每只
 * 股票、每个交易分钟保留最后一个 PriceTick，让分时图在压缩后仍能推进。
 * 已收盘交易日的分时点会被最后一个 DayBoundary 淘汰。成交带只显示有限条，
 * 因而同一刷新帧仅传最后 maxTrades 条。拒单和结算错误永不压缩。
 */
export function compactFastForwardEvents(
  events: EngineEvent[],
  maxTrades = 100,
  ticksPerMinute = 60,
  ticksPerDay = 14_400,
): EngineEvent[] {
  if (events.length === 0) return events;
  if (!Number.isSafeInteger(ticksPerMinute) || ticksPerMinute <= 0) {
    throw new RangeError("ticksPerMinute 必须是正整数");
  }
  if (!Number.isSafeInteger(ticksPerDay) || ticksPerDay <= 0) {
    throw new RangeError("ticksPerDay 必须是正整数");
  }

  const keep = new Set<number>();
  const tradeIndices: number[] = [];
  const latestActiveMinuteTick = new Map<string, number>();

  events.forEach((event, index) => {
    if ("DayBoundary" in event) {
      keep.add(index);
      latestActiveMinuteTick.clear();
      return;
    }
    if ("PriceTick" in event) {
      const dayTick = (Math.max(1, event.PriceTick.tick) - 1) % ticksPerDay;
      const minute = Math.floor(dayTick / ticksPerMinute);
      latestActiveMinuteTick.set(`${event.PriceTick.code}:${minute}`, index);
      return;
    }
    if ("Trade" in event) {
      tradeIndices.push(index);
      return;
    }
    keep.add(index);
  });

  for (const index of tradeIndices.slice(-Math.max(0, maxTrades))) keep.add(index);
  for (const index of latestActiveMinuteTick.values()) keep.add(index);

  return events.filter((_event, index) => keep.has(index));
}
