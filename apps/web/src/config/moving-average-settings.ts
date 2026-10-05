export interface MovingAverageSetting { readonly period: number; readonly visible: boolean }
export const MOVING_AVERAGE_STORAGE_KEY = "stock-game-moving-averages";
export const DEFAULT_MOVING_AVERAGES: readonly MovingAverageSetting[] = Object.freeze([5, 10, 20, 30, 60].map((period) => Object.freeze({ period, visible: true })));
export interface MovingAverageStorage { getItem(key: string): string | null; setItem(key: string, value: string): void }
export function parseMovingAverageSettings(value: unknown): readonly MovingAverageSetting[] {
  if (!Array.isArray(value)) throw new Error("MA设置必须为周期与显示标志列表");
  const seen = new Set<number>();
  return Object.freeze(value.map((item: unknown) => {
    if (typeof item !== "object" || item === null || Array.isArray(item) || Object.keys(item).length !== 2 || !Object.hasOwn(item, "period") || !Object.hasOwn(item, "visible")) throw new Error("每条MA设置必须恰有period与visible");
    const { period, visible } = item as Record<string, unknown>;
    if (typeof period !== "number" || !Number.isSafeInteger(period) || period <= 0 || seen.has(period)) throw new Error("MA周期必须为不重复的正安全整数");
    if (typeof visible !== "boolean") throw new Error("MA显示标志必须为boolean");
    seen.add(period);
    return Object.freeze({ period, visible });
  }));
}
export function loadMovingAverageSettings(storage: MovingAverageStorage): readonly MovingAverageSetting[] {
  const stored = storage.getItem(MOVING_AVERAGE_STORAGE_KEY);
  return stored === null ? DEFAULT_MOVING_AVERAGES : parseMovingAverageSettings(JSON.parse(stored));
}
export function saveMovingAverageSettings(storage: MovingAverageStorage, value: unknown): readonly MovingAverageSetting[] {
  const parsed = parseMovingAverageSettings(value);
  storage.setItem(MOVING_AVERAGE_STORAGE_KEY, JSON.stringify(parsed));
  return parsed;
}
