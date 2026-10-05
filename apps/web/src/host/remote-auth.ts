import type { SessionSetup } from "../types/engine.ts";
import { exact, record } from "./protocol/guards.ts";
import { canonicalU64, parseRemoteMarketContext, type RemoteMarketContext } from "./remote-market-context.ts";

export interface IdentitySubject {
  readonly subject_id: string;
  readonly username: string | null;
}

export interface RemoteLogin {
  readonly subject: IdentitySubject;
  readonly token: string;
}

export function credentialServerKey(value: string): string {
  let url: URL;
  try { url = new URL(value); } catch { throw new Error("Server 地址无效，请填写 HTTP(S) 地址"); }
  if (!["http:", "https:"].includes(url.protocol) || url.username !== "" || url.password !== "" || url.search !== "" || url.hash !== "") throw new Error("Server 地址必须为无账号、密码、查询或 fragment 的 HTTP(S) 地址");
  return url.toString().replace(/\/+$/, "");
}

export function parseCredentialToken(value: unknown): string {
  if (typeof value !== "string" || !/^[0-9a-f]{64}$/.test(value)) throw new Error("登录 credential 无效，请重新登录并反馈 remote-auth");
  return value;
}

export function parseIdentitySubject(value: unknown): IdentitySubject {
  const source = record(value, "登录身份");
  exact(source, ["subject_id", "username"], "登录身份");
  if (typeof source.subject_id !== "string" || source.subject_id.trim().length === 0 || (source.username !== null && (typeof source.username !== "string" || !/^[A-Za-z0-9_-]{1,64}$/.test(source.username)))) throw new Error("登录身份响应无效，请反馈 remote-auth");
  return { subject_id: source.subject_id, username: source.username as string | null };
}

export class RemoteAuthClient {
  readonly baseUrl: string;
  private readonly fetchFn: typeof fetch;
  constructor(baseUrl: string, fetchFn: typeof fetch = fetch) {
    this.baseUrl = credentialServerKey(baseUrl);
    this.fetchFn = fetchFn;
  }

  private async request(path: string, method: string, body?: unknown, token?: string, expectedStatus = 200): Promise<unknown> {
    const headers: Record<string, string> = {};
    if (body !== undefined) headers["content-type"] = "application/json";
    if (token !== undefined) headers.authorization = `Bearer ${parseCredentialToken(token)}`;
    let response: Response;
    try {
      response = await this.fetchFn(`${this.baseUrl}${path}`, { method, headers, ...(body === undefined ? {} : { body: JSON.stringify(body) }) });
    } catch { throw new Error(`remote-auth ${path} 网络请求失败，请检查 Server、HTTPS 与跨源配置并反馈`); }
    if (!response.ok) {
      const reason = response.status === 401 ? "凭据失效或用户名／密码错误，请重新登录；不会自动创建匿名身份" : response.status === 409 ? "用户名或市场状态冲突，请刷新列表或检查输入" : "Server 拒绝请求，请检查输入并反馈";
      throw new Error(`remote-auth ${path} HTTP ${response.status}：${reason}`);
    }
    if (response.status !== expectedStatus) throw new Error(`remote-auth ${path} 成功状态不符合契约，请反馈`);
    if (expectedStatus === 204) return null;
    try { return await response.json(); } catch { throw new Error(`remote-auth ${path} 响应不是合法 JSON，请反馈`); }
  }

  private async authenticate(kind: "register" | "login" | "guest", username?: string, password?: string): Promise<RemoteLogin> {
    if (kind !== "guest" && (typeof username !== "string" || !/^[A-Za-z0-9_-]{1,64}$/.test(username) || typeof password !== "string" || new TextEncoder().encode(password).length < 8 || new TextEncoder().encode(password).length > 1024)) throw new Error("用户名应为 1—64 个 ASCII 字母、数字、下划线或短横线；密码应为 8—1024 个 UTF-8 字节");
    const value = record(await this.request(`/api/auth/${kind}`, "POST", kind === "guest" ? {} : { username, password }, undefined, kind === "login" ? 200 : 201), "登录响应");
    exact(value, ["subject", "token"], "登录响应");
    return { subject: parseIdentitySubject(value.subject), token: parseCredentialToken(value.token) };
  }

  register(username: string, password: string): Promise<RemoteLogin> { return this.authenticate("register", username, password); }
  login(username: string, password: string): Promise<RemoteLogin> { return this.authenticate("login", username, password); }
  guest(): Promise<RemoteLogin> { return this.authenticate("guest"); }
  async me(token: string): Promise<IdentitySubject> { return parseIdentitySubject(await this.request("/api/auth/me", "GET", undefined, token)); }
  async logout(token: string): Promise<void> { await this.request("/api/auth/logout", "POST", {}, token, 204); }
  async markets(token: string): Promise<RemoteMarketContext[]> {
    const source = record(await this.request("/api/markets", "GET", undefined, token), "市场列表");
    exact(source, ["markets"], "市场列表");
    if (!Array.isArray(source.markets)) throw new Error("远程 markets 必须是数组");
    const markets = source.markets.map(parseRemoteMarketContext);
    if (new Set(markets.map((market) => market.session_id)).size !== markets.length) throw new Error("远程市场列表包含重复 session_id");
    return markets;
  }
  async context(token: string, sessionId: string): Promise<RemoteMarketContext> {
    return parseRemoteMarketContext(await this.request(`/api/market/context?${new URLSearchParams({ session_id: sessionId })}`, "GET", undefined, token));
  }
  async join(token: string, context: RemoteMarketContext, confirmedRejoin: boolean): Promise<RemoteMarketContext> {
    if (context.needs_rejoin && !confirmedRejoin) throw new Error("此身份不在当前存档中，请本人明确确认重新加入");
    return parseRemoteMarketContext(await this.request("/api/markets/join", "POST", { session_id: context.session_id, generation: context.generation, confirmed_rejoin: confirmedRejoin }, token));
  }
  async create(token: string, setup: SessionSetup, seed: string): Promise<RemoteMarketContext> {
    return parseRemoteMarketContext(await this.request("/api/new", "POST", { setup, seed: canonicalU64(seed, "创建市场 seed") }, token));
  }
}
