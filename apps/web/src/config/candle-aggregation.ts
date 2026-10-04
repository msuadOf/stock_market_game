import type { CandleAggregationBasis } from "../mobile/calendar-candles.ts";

export const CANDLE_AGGREGATION_STORAGE_KEY = "stock-game-candle-aggregation";

interface SettingsStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

export function parseCandleAggregationBasis(value: unknown): CandleAggregationBasis {
  if (value !== "calendar" && value !== "trading-days") throw new Error("K线聚合设置必须是calendar或trading-days");
  return value;
}

export function loadCandleAggregationBasis(storage: SettingsStorage): CandleAggregationBasis {
  const stored = storage.getItem(CANDLE_AGGREGATION_STORAGE_KEY);
  return stored === null ? "calendar" : parseCandleAggregationBasis(stored);
}

export function saveCandleAggregationBasis(storage: SettingsStorage, value: CandleAggregationBasis): void {
  storage.setItem(CANDLE_AGGREGATION_STORAGE_KEY, parseCandleAggregationBasis(value));
}
