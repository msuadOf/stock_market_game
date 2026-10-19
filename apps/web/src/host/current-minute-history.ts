/**
 * 当日分钟K查询归一层（W3 取回自 18725d07，按 B2 新 wire 契约适配）。
 *
 * ADR-0041 词表对齐：视图 code 串寻址；wire `StockId` 数值——翻译在
 * `wireCurrentMinuteHistoryRequest`（请求侧）与归一入口（响应侧）完成。
 *
 * 与旧 wire 的差异（B2 新基线）：
 * - `status: "Trading"|"Closed"` 改为 `trading: boolean`（二值同构）；
 * - `observed_at` 观察时刻字段不再随响应下发——实时"形成中"桶标记由
 *   面板调用方以游戏时钟（tick→墙钟分钟）提供，引擎面不加字段。
 */
import type { CivilDate } from "../types/generated/CivilDate.ts";
import type { CurrentMinuteHistoryPhase } from "../types/generated/CurrentMinuteHistoryPhase.ts";
import type { CurrentMinuteHistoryRequest as WireCurrentMinuteHistoryRequest } from "../types/generated/CurrentMinuteHistoryRequest.ts";
import { exact, field, record, values } from "./protocol/guards.ts";
import { stockCodeOf } from "./protocol/stock-identity.ts";
import { stockIdOf } from "../config/stock-ids.ts";
import { tradeHistoryDate } from "./personal-trade-history.ts";
import { normalizeMinuteBar } from "./market-history.ts";

/** 视图请求（代码串词表）。 */
export interface CurrentMinuteHistoryRequest {
  readonly code: string;
}

/** 视图响应（wire 同形——code 翻译后）。 */
export interface CurrentMinuteHistoryResponse {
  readonly code: string;
  readonly date: CivilDate;
  readonly live: true;
  readonly trading: boolean;
  readonly phase: CurrentMinuteHistoryPhase;
  readonly bars: readonly ReturnType<typeof normalizeMinuteBar>[];
}

/** 请求层翻译：视图 code 串 → wire 数值 StockId（wasm 请求侧，ADR-0041）。 */
export function wireCurrentMinuteHistoryRequest(request: CurrentMinuteHistoryRequest): WireCurrentMinuteHistoryRequest {
  return { stock: stockIdOf(request.code) };
}

export function normalizeCurrentMinuteHistoryRequest(value: unknown): CurrentMinuteHistoryRequest {
  const source = record(value, "当前分钟历史请求");
  exact(source, ["code"], "当前分钟历史请求");
  if (typeof source.code !== "string" || !/^\d{6}$/.test(source.code)) throw new Error("当前分钟证券必须为六位代码");
  return { code: source.code };
}

export function normalizeCurrentMinuteHistoryResponse(value: unknown, request: CurrentMinuteHistoryRequest): CurrentMinuteHistoryResponse {
  const expected = normalizeCurrentMinuteHistoryRequest(request), source = record(value, "当前分钟历史响应");
  // wire 的 stock 为数值 StockId（ADR 0041）；摄入时译回代码串再与请求比对。
  exact(source, ["stock", "date", "live", "trading", "phase", "bars"], "当前分钟历史响应");
  const wireStock = field(source, "stock", "当前分钟历史响应");
  if (typeof wireStock !== "number" || !Number.isInteger(wireStock)) throw new Error("当前分钟响应证券必须为数值StockId");
  const code = stockCodeOf(wireStock, "当前分钟历史响应.stock");
  if (code !== expected.code || source.live !== true) throw new Error("当前分钟响应证券错配或不是明确live事实");
  const date = tradeHistoryDate(source.date, "当前分钟.date");
  if (typeof source.trading !== "boolean") throw new Error("当前分钟开市状态必须是布尔值");
  const phase = source.phase;
  if (!["CallAuction", "PreOpen", "Continuous", "ClosingAuction", "AfterClose", "Closed"].includes(String(phase)) || (source.trading === false) !== (phase === "Closed")) throw new Error("当前分钟证券开市状态与交易阶段不同");
  const bars = values(source.bars, "bars").map(normalizeMinuteBar);
  let last = -1;
  for (const bar of bars) {
    if (bar.minute_of_day <= last) throw new Error("当前分钟成交乱序");
    last = bar.minute_of_day;
  }
  if (source.trading === false && bars.length !== 0) throw new Error("休市证券不能有当日成交分钟");
  return { code: expected.code, date, live: true, trading: source.trading, phase: phase as CurrentMinuteHistoryResponse["phase"], bars };
}
