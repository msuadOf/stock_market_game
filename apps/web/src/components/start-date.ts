import type { SessionSetup } from "../types/engine.ts";

export type StartDateParseResult =
  | { readonly kind: "valid"; readonly value: string }
  | { readonly kind: "invalid"; readonly message: string };

const START_DATE_MINIMUM = "2000-01-01";
const START_DATE_MAXIMUM = "2099-12-31";
const OUT_OF_RANGE_MESSAGE = "模拟起始日期必须在 2000-01-01 至 2099-12-31 之间";
const INVALID_DATE_MESSAGE = "模拟起始日期不是有效公历日";

function daysInMonth(year: number, month: number): number {
  if (month === 2) return year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0) ? 29 : 28;
  return [4, 6, 9, 11].includes(month) ? 30 : 31;
}

export function parseStartDate(value: string): StartDateParseResult {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value)) return { kind: "invalid", message: INVALID_DATE_MESSAGE };
  if (value < START_DATE_MINIMUM || value > START_DATE_MAXIMUM) {
    return { kind: "invalid", message: OUT_OF_RANGE_MESSAGE };
  }
  const [yearText, monthText, dayText] = value.split("-");
  const year = Number(yearText);
  const month = Number(monthText);
  const day = Number(dayText);
  if (month < 1 || month > 12 || day < 1 || day > daysInMonth(year, month)) {
    return { kind: "invalid", message: INVALID_DATE_MESSAGE };
  }
  return { kind: "valid", value };
}

export function setupWithStartDate(setup: SessionSetup, startDate: string): SessionSetup {
  return { ...setup, start_date: startDate };
}

export const DEFAULT_START_DATE = "2030-01-01";
