import type { MarketHistoryRequest, MarketHistoryPage } from "../host/market-history.ts";

export function shiftHistoryDate(date: string, days: number): string {
  const parsed = new Date(`${date}T00:00:00Z`);
  if (!/^\d{4}-\d{2}-\d{2}$/.test(date) || !Number.isFinite(parsed.getTime()) || parsed.toISOString().slice(0, 10) !== date) throw new Error(`历史自然日期无效：${date}`);
  parsed.setUTCDate(parsed.getUTCDate() + days);
  return parsed.toISOString().slice(0, 10);
}

export async function latestFiveTradingDays(query: (request: MarketHistoryRequest) => Promise<MarketHistoryPage>, code: string, currentDate: string): Promise<MarketHistoryPage> {
  const entries: MarketHistoryPage["entries"] = [];
  let end = shiftHistoryDate(currentDate, -1);
  let settled: string | null = null;
  while (entries.length < 5) {
    const start = shiftHistoryDate(end, -6);
    let after: string | null = null;
    let reachedStart = false;
    do {
      const page = await query({ code, date_from: start, date_to: end, after, page_size: 7 });
      settled = page.settled_through;
      for (const entry of page.entries) {
        if (entry.availability === "BeforeStart") reachedStart = true;
        if (entry.availability === "Traded" || entry.availability === "NoTrades") entries.push(entry);
      }
      if (page.next_cursor !== null && (page.next_cursor <= (after === null ? shiftHistoryDate(start, -1) : after) || page.next_cursor > end)) throw new Error("历史分页游标未向前推进或超出范围");
      after = page.next_cursor;
    } while (after !== null);
    if (reachedStart) break;
    end = shiftHistoryDate(start, -1);
  }
  entries.sort((left, right) => left.date.localeCompare(right.date));
  return { code, entries: entries.slice(-5), next_cursor: null, settled_through: settled };
}
