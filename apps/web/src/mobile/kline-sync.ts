import type { KlinePoint } from "../components/PriceChart";
import type { DailyCandleSnap, EngineEvent, Snapshot } from "../types/engine";

export interface SyncedCandles {
  completed: Record<string, KlinePoint[]>;
  active: Record<string, KlinePoint>;
}

export function toChartCandle(candle: DailyCandleSnap): KlinePoint {
  return {
    time: candle.time as KlinePoint["time"],
    open: candle.open / 100,
    high: candle.high / 100,
    low: candle.low / 100,
    close: candle.close / 100,
    volume: candle.volume,
    ...(candle.trade_stats === undefined || candle.trade_stats === null ? {} : {
      tradeStats: {
        turnoverCents: candle.trade_stats.turnover_cents,
        tradeCount: candle.trade_stats.trade_count,
      },
    }),
  };
}

export interface CandleEventState extends SyncedCandles {
  changedCodes: Set<string>;
}

/**
 * 只归并 Rust 事件里携带的权威蜡烛。高倍速批次只转换每股最后一根盘中 K，
 * 日界则直接提交 Rust 给出的收盘 K，避免前端从 Trade/PriceTick 重算行情。
 */
export function reduceCandleEvents(
  completed: Record<string, KlinePoint[]>,
  active: Record<string, KlinePoint>,
  events: EngineEvent[],
  historyLimit = 360,
): CandleEventState {
  if (!Number.isSafeInteger(historyLimit) || historyLimit <= 0) {
    throw new RangeError(`invalid K-line history limit: ${historyLimit}`);
  }
  let nextCompleted = completed;
  const nextActive = { ...active };
  const pendingActive = new Map<string, DailyCandleSnap>();
  const changedCodes = new Set<string>();

  for (const event of events) {
    if ("PriceTick" in event) {
      pendingActive.set(event.PriceTick.code, event.PriceTick.daily_candle);
      changedCodes.add(event.PriceTick.code);
      continue;
    }
    if (!("DayBoundary" in event)) continue;

    if (nextCompleted === completed) nextCompleted = { ...completed };
    for (const [code, candle] of Object.entries(event.DayBoundary.closed_daily_candles)) {
      const chartCandle = toChartCandle(candle);
      const history = nextCompleted[code] ?? [];
      const withoutDuplicate = history.at(-1)?.time === chartCandle.time ? history.slice(0, -1) : history;
      nextCompleted[code] = [...withoutDuplicate, chartCandle].slice(-historyLimit);
      delete nextActive[code];
      pendingActive.delete(code);
      changedCodes.add(code);
    }
  }

  for (const [code, candle] of pendingActive) nextActive[code] = toChartCandle(candle);
  return { completed: nextCompleted, active: nextActive, changedCodes };
}

/** 只做协议单位转换；绝不在 Web 端生成、补齐或改写行情历史。 */
export function candlesFromSnapshot(snapshot: Snapshot): SyncedCandles {
  return {
    completed: Object.fromEntries(
      Object.entries(snapshot.daily_candles).map(([code, candles]) => [
        code,
        candles.map(toChartCandle),
      ]),
    ),
    active: Object.fromEntries(
      Object.entries(snapshot.active_daily_candles).map(([code, candle]) => [
        code,
        toChartCandle(candle),
      ]),
    ),
  };
}
