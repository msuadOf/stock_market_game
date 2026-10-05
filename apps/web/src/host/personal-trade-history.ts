import type { PersonalTradeHistoryRequest } from "../types/generated/PersonalTradeHistoryRequest";
import type { PersonalTradeHistoryPage } from "../types/generated/PersonalTradeHistoryPage";
import { normalizeConfirmationCursor, normalizePersonalTradeConfirmations } from "./intraday-average.ts";

function record(value: unknown, path: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error(`${path} 必须为对象`);
  return value as Record<string, unknown>;
}

function exact(value: Record<string, unknown>, fields: readonly string[], path: string): void {
  const missing = fields.find((field) => !Object.hasOwn(value, field));
  const extra = Object.keys(value).find((field) => !fields.includes(field));
  if (missing !== undefined || extra !== undefined) throw new Error(`${path}.${missing ?? extra} 不是完整当前契约`);
}

export function tradeHistoryDate(value: unknown, path: string): string {
  if (typeof value !== "string" || !/^(19|20|21)\d{2}-(0[1-9]|1[0-2])-\d{2}$/.test(value) || new Date(`${value}T00:00:00.000Z`).toISOString().slice(0, 10) !== value) throw new Error(`${path} 必须为1900–2199有效自然日`);
  return value;
}

function cursor(value: unknown, path: string): string | null {
  if (value !== null && typeof value !== "string") throw new Error(`${path} 必须为规范u64字符串或null`);
  return normalizeConfirmationCursor(value);
}

export function normalizePersonalTradeHistoryRequest(value: unknown): PersonalTradeHistoryRequest {
  const item = record(value, "本人日期查询");
  exact(item, ["date_from", "date_to", "code", "side", "before_receipt", "as_of_receipt", "page_size"], "本人日期查询");
  const from = tradeHistoryDate(item.date_from, "date_from");
  const to = tradeHistoryDate(item.date_to, "date_to");
  if (from > to) throw new Error("本人日期范围起日不能晚于止日");
  if (item.code !== null && (typeof item.code !== "string" || !/^\d{6}$/.test(item.code))) throw new Error("本人证券筛选必须为六位代码或null");
  if (item.side !== null && item.side !== "Buy" && item.side !== "Sell") throw new Error("本人方向筛选必须为Buy、Sell或null");
  if (typeof item.page_size !== "number" || !Number.isSafeInteger(item.page_size) || item.page_size < 1 || item.page_size > 100) throw new Error("本人日期查询每页须为1–100条，可继续翻页完整读取");
  const before = cursor(item.before_receipt, "before_receipt");
  const ceiling = cursor(item.as_of_receipt, "as_of_receipt");
  if (ceiling !== null && before !== null && BigInt(before) > BigInt(ceiling)) throw new Error("before_receipt不能超过固定receipt窗口");
  return { date_from: from, date_to: to, code: item.code, side: item.side, before_receipt: before, as_of_receipt: ceiling, page_size: item.page_size };
}

export function normalizePersonalTradeHistoryPage(value: unknown, request: PersonalTradeHistoryRequest): PersonalTradeHistoryPage {
  const expected = normalizePersonalTradeHistoryRequest(request);
  const item = record(value, "本人日期页");
  exact(item, ["request", "confirmations", "next_cursor", "as_of_receipt", "start_date", "current_date", "settled_through"], "本人日期页");
  const echoed = normalizePersonalTradeHistoryRequest(item.request);
  if (JSON.stringify(echoed) !== JSON.stringify(expected)) throw new Error("本人日期页请求与当前filters不一致");
  const ceiling = cursor(item.as_of_receipt, "as_of_receipt");
  if (ceiling === null || (expected.as_of_receipt !== null && ceiling !== expected.as_of_receipt)) throw new Error("本人日期页固定receipt窗口与请求不一致");
  const next = cursor(item.next_cursor, "next_cursor");
  const rows = normalizePersonalTradeConfirmations(item.confirmations);
  if (rows.length > expected.page_size) throw new Error("本人日期页超过所请求page_size");
  let previous = expected.before_receipt === null ? BigInt(ceiling) : BigInt(expected.before_receipt);
  for (const row of rows) {
    const receipt = BigInt(row.receipt_id);
    if (receipt >= BigInt(ceiling) || receipt >= previous || row.civil_date < expected.date_from || row.civil_date > expected.date_to || (expected.code !== null && row.code !== expected.code) || (expected.side !== null && row.side !== expected.side)) throw new Error("本人日期页成交事实超出日期/筛选/receipt范围或顺序错误");
    previous = receipt;
  }
  if (next !== null && (rows.length !== expected.page_size || next !== rows.at(-1)?.receipt_id)) throw new Error("本人日期页next_cursor必须指向完整页的最后receipt");
  const start = tradeHistoryDate(item.start_date, "start_date");
  const current = tradeHistoryDate(item.current_date, "current_date");
  const settled = item.settled_through === null ? null : tradeHistoryDate(item.settled_through, "settled_through");
  if (start > current || (settled !== null && (settled < start || settled >= current))) throw new Error("本人日期页起日/当前日/已结束自然日元数据不一致");
  if (rows.some((row) => row.civil_date < start || row.civil_date > current)) throw new Error("本人日期页交割事实超出本局已发生自然日范围");
  return { request: echoed, confirmations: [...rows], next_cursor: next, as_of_receipt: ceiling, start_date: start, current_date: current, settled_through: settled };
}
