import type { EngineEvent } from "../types/engine";
import { AUCTION_VOLUME_LINES_PER_MINUTE, CALL_AUCTION_TICKS, TICKS_PER_TRADING_MINUTE, TOTAL_TICKS_PER_DAY } from "../config/defaults.ts";

/** UI 背压只能阻止发消息，不能阻止引擎继续推进。 */
export function uiBackpressurePolicy(awaitingUiFrame: boolean): {
  stepEngine: true;
  flushUi: boolean;
} {
  return { stepEngine: true, flushUi: !awaitingUiFrame };
}

/** 统一 serde-wasm-bindgen 与 JSON 的 Map / Option 表示，保持前端事件契约稳定。 */
export function normalizeEventMaps(events: EngineEvent[]): EngineEvent[] {
  let normalized: EngineEvent[] | null = null;
  events.forEach((event, index) => {
    let replacement: EngineEvent | null = null;
    if ("AuctionTick" in event && event.AuctionTick.indicative_price === undefined) {
      replacement = { AuctionTick: { ...event.AuctionTick, indicative_price: null } };
    } else if ("AuctionCompleted" in event && event.AuctionCompleted.clearing_price === undefined) {
      replacement = { AuctionCompleted: { ...event.AuctionCompleted, clearing_price: null } };
    } else if ("DayBoundary" in event && event.DayBoundary.closed_daily_candles instanceof Map) {
      replacement = {
        DayBoundary: {
          ...event.DayBoundary,
          closed_daily_candles: Object.fromEntries(event.DayBoundary.closed_daily_candles),
        },
      };
    }
    if (!replacement) return;
    normalized ??= [...events];
    normalized[index] = replacement;
  });
  return normalized ?? events;
}

/** 单线程与 Worker 宿主共用的 WASM 事件入站边界。 */
export function normalizeWasmStepEvents(value: unknown): EngineEvent[] {
  if (!Array.isArray(value)) {
    throw new TypeError("WASM step 返回值必须是事件数组");
  }
  return normalizeEventMaps(value as EngineEvent[]);
}

/**
 * “最快”只描述引擎推进速度，不代表 React 必须渲染每个中间 tick。
 *
 * 跨日事件携带完整的权威收盘 K 线，必须全部保留；最后一个交易日内，每只
 * 股票、每个交易分钟保留最后一个 PriceTick；集合竞价每六秒保留最后一个
 * AuctionTick，让分时图和累计量细线在压缩后仍能推进。
 * 已收盘交易日的分时点会被最后一个 DayBoundary 淘汰。成交带只显示有限条，
 * 因而同一刷新帧仅传最后 maxTrades 条。拒单和结算错误永不压缩。
 */
export function compactFastForwardEvents(
  events: EngineEvent[],
  maxTrades = 100,
  ticksPerMinute = TICKS_PER_TRADING_MINUTE,
  ticksPerDay = TOTAL_TICKS_PER_DAY,
  auctionTicks = CALL_AUCTION_TICKS,
): EngineEvent[] {
  if (events.length === 0) return events;
  if (!Number.isSafeInteger(ticksPerMinute) || ticksPerMinute <= 0) {
    throw new RangeError("ticksPerMinute 必须是正整数");
  }
  if (!Number.isSafeInteger(ticksPerDay) || ticksPerDay <= 0) {
    throw new RangeError("ticksPerDay 必须是正整数");
  }
  if (!Number.isSafeInteger(auctionTicks) || auctionTicks < 0 || auctionTicks >= ticksPerDay) {
    throw new RangeError("auctionTicks 必须是小于 ticksPerDay 的非负整数");
  }

  const keep = new Set<number>();
  const tradeIndices: number[] = [];
  const latestActiveMinuteTick = new Map<string, number>();
  const latestAuctionMinuteTick = new Map<string, number>();

  events.forEach((event, index) => {
    if ("DayBoundary" in event) {
      keep.add(index);
      latestActiveMinuteTick.clear();
      latestAuctionMinuteTick.clear();
      return;
    }
    if ("AuctionTick" in event) {
      const dayTick = (Math.max(1, event.AuctionTick.tick) - 1) % ticksPerDay;
      const lineSlot = Math.floor(dayTick / (ticksPerMinute / AUCTION_VOLUME_LINES_PER_MINUTE));
      latestAuctionMinuteTick.set(`${event.AuctionTick.code}:${lineSlot}`, index);
      return;
    }
    if ("AuctionCompleted" in event) {
      keep.add(index);
      return;
    }
    if ("PriceTick" in event) {
      const dayTick = (Math.max(1, event.PriceTick.tick) - 1) % ticksPerDay;
      const minute = Math.floor((dayTick - auctionTicks) / ticksPerMinute);
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
  for (const index of latestAuctionMinuteTick.values()) keep.add(index);

  return events.filter((_event, index) => keep.has(index));
}
