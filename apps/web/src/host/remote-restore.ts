import { observerOrdinal } from "./observer-resync.ts";
export class RemoteRestoreFailure extends Error {
  constructor(readonly outcome: "rejected" | "unknown", message: string, options?: ErrorOptions) { super(message, options); }
}
export async function remoteRestoreJson(fetcher: typeof fetch, url: string, init: RequestInit): Promise<unknown> {
  let response: Response;
  try { response = await fetcher(url, init); }
  catch (error) { throw new RemoteRestoreFailure("unknown", "远程读档请求中断，提交结果未知", { cause: error }); }
  const outcome = response.headers.get("x-session-restore-outcome");
  let value: unknown;
  try { value = JSON.parse(await response.text()); }
  catch (error) { throw new RemoteRestoreFailure(outcome === "rejected" ? "rejected" : "unknown", "远程读档回执无法解码", { cause: error }); }
  if (!response.ok) throw new RemoteRestoreFailure(outcome === "rejected" ? "rejected" : "unknown", `远程读档失败（HTTP ${response.status}）：${JSON.stringify(value)}`);
  if (outcome !== "committed") throw new RemoteRestoreFailure("unknown", "已协商的远程读档缺少提交回执");
  return value;
}
export function parseRestoreCommit(value: unknown, _previous: string | null): string {
  if (value === null || typeof value !== "object" || !("outcome" in value) || value.outcome !== "committed" || !("replacement" in value)) throw new RemoteRestoreFailure("unknown", "读档提交回执无效");
  const replacement = value.replacement;
  if (replacement === null || typeof replacement !== "object" || !("previous_generation" in replacement) || !("generation" in replacement)) throw new RemoteRestoreFailure("unknown", "读档替换身份无效");
  const before = observerOrdinal(replacement.previous_generation), after = observerOrdinal(replacement.generation);
  // 回执证明本次提交；其他controller可使本地基线领先或落后，不改变已提交事实。
  if (BigInt(before) < 1n || BigInt(after) !== BigInt(before) + 1n) throw new RemoteRestoreFailure("unknown", "读档替换身份与请求代际不匹配");
  return after;
}
