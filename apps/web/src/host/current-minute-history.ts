import type { CurrentMinuteHistoryRequest } from "../types/generated/CurrentMinuteHistoryRequest";
import type { CurrentMinuteHistoryResponse } from "../types/generated/CurrentMinuteHistoryResponse";
import { exact, record, values } from "./protocol/guards.ts";
import { tradeHistoryDate } from "./personal-trade-history.ts";
import { normalizeMinuteBar } from "./market-history.ts";
export type { CurrentMinuteHistoryRequest, CurrentMinuteHistoryResponse };

export function normalizeCurrentMinuteHistoryRequest(value: unknown): CurrentMinuteHistoryRequest {
  const source = record(value, "当前分钟历史请求");
  exact(source, ["code"], "当前分钟历史请求");
  if (typeof source.code !== "string" || !/^\d{6}$/.test(source.code)) throw new Error("当前分钟证券必须为六位代码");
  return { code: source.code };
}

export function normalizeCurrentMinuteHistoryResponse(value: unknown, request: CurrentMinuteHistoryRequest): CurrentMinuteHistoryResponse {
  const expected = normalizeCurrentMinuteHistoryRequest(request), source = record(value, "当前分钟历史响应");
  exact(source, ["code", "date", "observed_at", "live", "status", "phase", "bars"], "当前分钟历史响应");
  if (source.code !== expected.code || source.live !== true) throw new Error("当前分钟响应证券错配或不是明确live事实");
  const date = tradeHistoryDate(source.date, "当前分钟.date");
  const observed = record(source.observed_at, "observed_at");
  exact(observed, ["date", "second_of_day"], "observed_at");
  if (tradeHistoryDate(observed.date, "observed_at.date") !== date || typeof observed.second_of_day !== "number" || !Number.isInteger(observed.second_of_day) || observed.second_of_day < 0 || observed.second_of_day >= 86400) throw new Error("当前分钟自然日期与观察时刻不一致");
  if (source.status !== "Trading" && source.status !== "Closed") throw new Error("当前分钟证券开市状态无效");
  const phase = source.phase;
  if (!["CallAuction", "PreOpen", "Continuous", "ClosingAuction", "AfterClose", "Closed"].includes(String(phase)) || (source.status === "Closed") !== (phase === "Closed")) throw new Error("当前分钟证券开市状态与交易阶段不同");
  const bars = values(source.bars, "bars").map(normalizeMinuteBar);
  let last = -1;
  for (const bar of bars) {
    if (bar.minute_of_day <= last || bar.minute_of_day > Math.floor(observed.second_of_day / 60)) throw new Error("当前分钟成交乱序或晚于真实观察时刻");
    last = bar.minute_of_day;
  }
  if (source.status === "Closed" && bars.length !== 0) throw new Error("休市证券不能有当日成交分钟");
  return { code: expected.code, date, observed_at: { date, second_of_day: observed.second_of_day }, live: true, status: source.status, phase: phase as CurrentMinuteHistoryResponse["phase"], bars };
}
