import type { CalendarCandlePeriod } from "../mobile/calendar-candles.ts";

export const MINUTE_KLINE_PERIODS = [120, 60, 30, 15, 5, 1] as const;
export type MinuteKlinePeriod = typeof MINUTE_KLINE_PERIODS[number];
export type KlinePeriod = CalendarCandlePeriod | `${MinuteKlinePeriod}分钟K`;
export type ChartPeriod = "分时" | "五日" | KlinePeriod;
export const KLINE_PERIODS: readonly KlinePeriod[] = ["日K", "周K", "月K", "季K", "年K", ...MINUTE_KLINE_PERIODS.map((period) => `${period}分钟K` as const)];

export function parseKlinePeriod(value: string): KlinePeriod {
  if (!KLINE_PERIODS.includes(value as KlinePeriod)) throw new Error(`未知K线周期：${value}`);
  return value as KlinePeriod;
}

export function minuteKlinePeriod(period: ChartPeriod): MinuteKlinePeriod | null {
  const match = MINUTE_KLINE_PERIODS.find((value) => `${value}分钟K` === period);
  return match === undefined ? null : match;
}

export function calendarCandlePeriod(period: ChartPeriod): CalendarCandlePeriod | null {
  return ["日K", "周K", "月K", "季K", "年K"].includes(period) ? period as CalendarCandlePeriod : null;
}
