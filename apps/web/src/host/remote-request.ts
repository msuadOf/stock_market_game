import type { DeliveryMode } from "./engine-host.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";

export function remoteBaseUrl(value: string): string {
  const baseUrl = value.trim().replace(/\/+$/, "");
  if (!/^https?:\/\//i.test(baseUrl)) throw new Error(`远程服务地址必须以 http:// 或 https:// 开头：${value}`);
  return baseUrl;
}

export function remoteWsUrl(baseUrl: string, sessionId: string, token: string, delivery: DeliveryMode): string {
  const url = new URL(`${baseUrl}/ws`);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.searchParams.set("session_id", sessionId);
  url.searchParams.set("delivery", delivery);
  url.searchParams.set("token", token);
  return url.toString();
}

export async function remoteJson(fetchFn: typeof fetch, url: string, init: RequestInit): Promise<unknown> {
  const response = await fetchFn(url, init);
  const text = await response.text();
  if (!response.ok) throw new Error(`远程服务请求失败（HTTP ${response.status}）：${text || response.statusText}`);
  if (text.length === 0) return null;
  try {
    return JSON.parse(text);
  } catch (error) {
    throw new Error(`远程服务成功响应不是合法 JSON：${error instanceof Error ? error.message : String(error)}`);
  }
}

export function remotePost(body: unknown, token: string | null = null): RequestInit {
  const headers: Record<string, string> = { "content-type": "application/json" };
  if (token !== null) headers.authorization = `Bearer ${token}`;
  return { method: "POST", headers, body: JSON.stringify(body) };
}

export function remotePausePreferencePayload(
  sessionId: string,
  generation: string,
  preferences: PausePreferences,
): { readonly session_id: string; readonly generation: string; readonly preferences: PausePreferences } {
  return { session_id: sessionId, generation, preferences };
}

export function remoteSession(value: unknown): { readonly id: string; readonly token: string | null } {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("远程建会话响应必须是对象");
  const source = value as Readonly<Record<string, unknown>>;
  if (typeof source.session_id !== "string" || source.session_id.length === 0) throw new Error("远程服务没有返回合法 session_id");
  if (source.session_token !== undefined && typeof source.session_token !== "string") throw new Error("远程服务返回的 session_token 无效");
  return { id: source.session_id, token: typeof source.session_token === "string" ? source.session_token : null };
}
