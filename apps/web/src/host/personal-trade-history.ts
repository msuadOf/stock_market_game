/**
 * 个人成交历史查询归一层（W3 取回自 18725d07，按 B2 新 wire 契约适配）。
 *
 * ADR-0041 词表对齐：视图筛选用六位代码串（`code`）；wire 过滤项为数值
 * `stock: StockId | null`——翻译在 `wirePersonalTradeHistoryRequest`（请求侧）
 * 与响应回显归一（响应侧）完成。旧 wire 的 `price`/`gross` 金额字段在 B2
 * 重建后为 `price_cents`/`gross_cents`（同口径十进制分字符串，仅更名）。
 */
import type { CivilDate } from "../types/generated/CivilDate.ts";
import type { Side } from "../types/generated/Side.ts";
import type { PersonalTradeHistoryRequest as WirePersonalTradeHistoryRequest } from "../types/generated/PersonalTradeHistoryRequest.ts";
import { normalizeConfirmationCursor, normalizePersonalTradeConfirmations } from "./intraday-average.ts";
import { stockCodeOf } from "./protocol/stock-identity.ts";
import { stockIdOf } from "../config/stock-ids.ts";

/** 视图请求（代码串词表——与旧 main 视图契约一致）。 */
export interface PersonalTradeHistoryRequest {
  readonly date_from: CivilDate;
  readonly date_to: CivilDate;
  readonly code: string | null;
  readonly side: Side | null;
  readonly before_receipt: string | null;
  readonly as_of_receipt: string | null;
  readonly page_size: number;
}

/** 视图分页结果。 */
export interface PersonalTradeHistoryPage {
  readonly request: PersonalTradeHistoryRequest;
  readonly confirmations: readonly ReturnType<typeof normalizePersonalTradeConfirmations>[number][];
  readonly next_cursor: string | null;
  readonly as_of_receipt: string;
  readonly start_date: CivilDate;
  readonly current_date: CivilDate;
  readonly settled_through: CivilDate | null;
}

/** 请求层翻译：视图 code 串 → wire 数值 StockId（wasm 请求侧，ADR-0041）。 */
export function wirePersonalTradeHistoryRequest(request: PersonalTradeHistoryRequest): WirePersonalTradeHistoryRequest {
  return {
    date_from: request.date_from,
    date_to: request.date_to,
    stock: request.code === null ? null : stockIdOf(request.code),
    side: request.side,
    before_receipt: request.before_receipt,
    as_of_receipt: request.as_of_receipt,
    page_size: request.page_size,
  };
}

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

/** wire 回显请求 → 视图请求（数值 stock 译回代码串，ADR-0041——键位
 *  随词表替换：wire `stock` 在视图形态中不存在，剥离后再走视图校验）。 */
function normalizeWireEchoedRequest(value: unknown): PersonalTradeHistoryRequest {
  const item = record(value, "本人日期页.request");
  exact(item, ["date_from", "date_to", "stock", "side", "before_receipt", "as_of_receipt", "page_size"], "本人日期页.request");
  if (item.stock !== null && (typeof item.stock !== "number" || !Number.isInteger(item.stock))) throw new Error("本人日期页.request.stock 必须为数值StockId或null");
  const code = item.stock === null ? null : stockCodeOf(item.stock, "本人日期页.request.stock");
  const { stock: _wireStock, ...viewFields } = item;
  return normalizePersonalTradeHistoryRequest({ ...viewFields, code });
}

export function normalizePersonalTradeHistoryPage(value: unknown, request: PersonalTradeHistoryRequest): PersonalTradeHistoryPage {
  const expected = normalizePersonalTradeHistoryRequest(request);
  const item = record(value, "本人日期页");
  exact(item, ["request", "confirmations", "next_cursor", "as_of_receipt", "start_date", "current_date", "settled_through"], "本人日期页");
  const echoed = normalizeWireEchoedRequest(item.request);
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
