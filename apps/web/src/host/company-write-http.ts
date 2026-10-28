import { CompanyWriteRejectedError, parseCompanyWriteFailure } from "./company-write.ts";
import { parseRawSaveJson } from "../save/schema/raw-json.ts";
import { exact, record } from "../save/schema/primitives.ts";

/** 只供三项公司写命令使用，保公开 typed failure 与原始 HTTP 详情。 */
export async function companyWriteHttpJson(fetchFn: typeof fetch, url: string, init: RequestInit, rejectedCode: "PLAYER_COMPANY_ACTION_REJECTED" | "REPORT_CORRECTION_REJECTED"): Promise<unknown> {
  const response = await fetchFn(url, init);
  const text = await response.text();
  let raw: unknown;
  try { raw = parseRawSaveJson(text); }
  catch (error) { throw new Error(`公司写命令 HTTP ${response.status} 响应不是严格 JSON`, { cause: { responseText: text, decoderError: error } }); }
  if (response.ok) return raw;
  if (raw !== null && typeof raw === "object" && Object.hasOwn(raw, "failure")) {
    let failure;
    try {
      const envelope = record(raw, "company_write.http");
      exact(envelope, ["code", "failure"], "company_write.http");
      if (envelope.code !== rejectedCode) throw new Error("公司写命令 HTTP 拒绝 code 与命令不一致");
      failure = parseCompanyWriteFailure(envelope.failure);
    } catch (decoderError) {
      throw new AggregateError([decoderError], "公司写命令 HTTP typed 拒绝不符合协议", { cause: { status: response.status, response: raw, responseText: text } });
    }
    throw new CompanyWriteRejectedError(failure);
  }
  // 鉴权、过期请求与真正 Step 等非业务错误不转换为 CompanyWriteRejectedError。
  throw new Error(`公司写命令 HTTP ${response.status} 被拒绝：${text}`, { cause: { status: response.status, response: raw, responseText: text } });
}
