import type { Intent, PublicReportPage, PublicReportQuery, PublicReportSummary, SessionSetup, Snapshot } from "../types/engine.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import type { DeliveryMode, EngineHost } from "./engine-host.ts";
import type { HostFailure, HostUpdate } from "./host-update.ts";
import { UI_TARGET_HZ } from "./host-update.ts";
import { remoteBaseUrl, remoteJson, remotePausePreferencePayload, remotePost, remoteSession, remoteWsUrl } from "./remote-request.ts";
import { parseRemoteMessage } from "./remote-wire.ts";
import { normalizePublicReportById, normalizePublicReportPage } from "./serde-normalize.ts";
import { assertValidSpeedMultiplier, parseSpeedMetrics } from "./speed.ts";
import { normalizePlayerWorkingOrders, type PlayerWorkingOrder } from "./player-working-orders.ts";
import { parseNpcDecisionDiagnostics, type NpcDecisionTraceRecord } from "./npc-decision-trace.ts";
import type { IndicatorInput, IndicatorResults } from "../components/indicator-results.ts";
import { normalizeIndicatorInput, normalizeIndicatorResults } from "./indicator-transport.ts";

type RemoteHostOptions = {
  readonly baseUrl?: string;
  readonly token?: string;
  readonly fetchFn?: typeof fetch;
  readonly webSocketFactory?: (url: string) => WebSocket;
};

export function remoteSpeedValue(multiplier: number): number | "Fastest" {
  assertValidSpeedMultiplier(multiplier);
  return multiplier === Infinity ? "Fastest" : multiplier;
}

export async function createRemoteHost(setup: SessionSetup, seed: bigint, options: RemoteHostOptions = {}): Promise<EngineHost> {
  const environment = import.meta.env ?? {};
  const baseUrl = remoteBaseUrl(options.baseUrl ?? environment.VITE_REMOTE_BASE_URL ?? "http://127.0.0.1:3000");
  const fetchFn = options.fetchFn ?? fetch;
  const created = remoteSession(await remoteJson(fetchFn, `${baseUrl}/api/new`, remotePost({ setup, seed: seed.toString() })));
  const token = options.token ?? (environment.DEV === true ? environment.VITE_REMOTE_TOKEN : undefined) ?? created.token ?? "local-player";
  if (token === null || token.length === 0) throw new Error("远程会话缺少授权 token");
  const capabilityParams = new URLSearchParams({ session_id: created.id });
  const rawCapabilities = await remoteJson(fetchFn, `${baseUrl}/api/host-capabilities?${capabilityParams}`, {
    method: "GET",
    headers: { authorization: `Bearer ${token}` },
  });
  if (rawCapabilities === null || typeof rawCapabilities !== "object" || Array.isArray(rawCapabilities)) throw new Error("远程宿主能力响应必须是对象");
  const capabilityRecord = rawCapabilities as Record<string, unknown>;
  if (Object.keys(capabilityRecord).length !== 1 || typeof capabilityRecord.npcDecisionDiagnostics !== "boolean") throw new Error("远程宿主能力响应不符合契约");
  const socketFactory = options.webSocketFactory ?? ((url: string) => new WebSocket(url));
  let socket: WebSocket | null = null;
  let callback: ((update: HostUpdate) => void) | null = null;
  let fatalCallback: ((failure: HostFailure) => void) | null = null;
  let cachedBaseline: Extract<HostUpdate, { type: "baseline" }> | null = null;
  let baselineEpoch = 0;
  let running = false;
  let disposed = false;
  let delivery: DeliveryMode = "push";
  let connectionGeneration = 0;
  let awaitingBaseline = true;
  let requestSequence = 0;
  const reportCompanies = new Map<string, string>();
  let reportQueryEpoch = 0n;
  const pendingCommands = new Map<number, { readonly resolve: () => void; readonly reject: (error: Error) => void }>();
  let baselineWaiter: { readonly resolve: () => void; readonly reject: (error: Error) => void } | null = null;

  const fail = (failure: HostFailure) => {
    running = false;
    const failedSocket = socket;
    const waiter = baselineWaiter;
    socket = null;
    baselineWaiter = null;
    waiter?.reject(new Error(`${failure.code}: ${failure.message}`));
    failedSocket?.close();
    for (const pending of pendingCommands.values()) pending.reject(new Error(`${failure.code}: ${failure.message}`));
    pendingCommands.clear();
    fatalCallback?.(failure);
  };

  const requestResync = (): Promise<void> => {
    const current = socket;
    if (current === null || current.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error("远程 Publisher 未连接，无法请求基线重同步"));
    }
    awaitingBaseline = true;
    current.send(JSON.stringify({ Resync: {} }));
    return new Promise((resolve, reject) => { baselineWaiter = { resolve, reject }; });
  };

  const handle = (raw: string, identity: number) => {
    if (identity !== connectionGeneration || disposed) return;
    const parsed = parseRemoteMessage(raw, cachedBaseline?.generation ?? null);
    switch (parsed.kind) {
      case "baseline":
        baselineEpoch += 1;
        if (cachedBaseline !== null && cachedBaseline.generation !== parsed.update.generation) {
          reportQueryEpoch += 1n;
          reportCompanies.clear();
        }
        cachedBaseline = parsed.update;
        awaitingBaseline = false;
        callback?.(parsed.update);
        baselineWaiter?.resolve();
        baselineWaiter = null;
        return;
      case "protocol":
        if (awaitingBaseline || cachedBaseline === null) return;
        if (parsed.update.generation !== cachedBaseline.generation) {
          void requestResync().catch((error) => fail({ code: "REMOTE_RESYNC", where: "remote-host.generation", message: error instanceof Error ? error.message : String(error) }));
          return;
        }
        callback?.(parsed.update);
        return;
      case "failure":
        fail(parsed.failure);
        return;
      case "resync":
        void requestResync().catch((error) => fail({ code: "REMOTE_RESYNC", where: "remote-host.resync", message: `${parsed.message}；${error instanceof Error ? error.message : String(error)}` }));
        return;
      case "empty":
        return;
      case "queued": {
        const pending = pendingCommands.get(parsed.requestId);
        if (pending === undefined) throw new Error(`收到未知写请求 ${parsed.requestId} 的入队确认`);
        pendingCommands.delete(parsed.requestId);
        pending.resolve();
        return;
      }
      case "gateway-error": {
        if (parsed.requestId === null) {
          fail(parsed.failure);
          return;
        }
        const pending = pendingCommands.get(parsed.requestId);
        if (pending === undefined) throw new Error(`收到未知写请求 ${parsed.requestId} 的网关错误`);
        pendingCommands.delete(parsed.requestId);
        pending.reject(new Error(`${parsed.failure.code}: ${parsed.failure.message}`));
        return;
      }
      default:
        return assertNever(parsed);
    }
  };

  const connect = () => {
    if (socket !== null || disposed) return;
    const identity = ++connectionGeneration;
    const next = socketFactory(remoteWsUrl(baseUrl, created.id, token, delivery));
    socket = next;
    next.onmessage = (event) => {
      try {
        handle(String(event.data), identity);
      } catch (error) {
        fail({ code: "REMOTE_PROTOCOL", where: "remote-host.message", message: error instanceof Error ? error.message : String(error) });
      }
    };
    next.onerror = () => fail({ code: "REMOTE_SOCKET", where: "remote-host.socket", message: "远程 WebSocket 连接发生错误" });
    next.onclose = () => {
      if (socket === next) socket = null;
      if ((running || baselineWaiter !== null) && !disposed && identity === connectionGeneration) {
        fail({ code: "REMOTE_SOCKET", where: "remote-host.socket", message: "远程 WebSocket 意外断开" });
      }
    };
  };

  const connectRestoredBaseline = (): Promise<void> => new Promise((resolve, reject) => {
    awaitingBaseline = true;
    const timeout = setTimeout(() => fail({ code: "REMOTE_BASELINE_TIMEOUT", where: "remote-host.load", message: "读取恢复后的远程权威基线超时（5000ms）" }), 5000);
    baselineWaiter = {
      resolve() { clearTimeout(timeout); resolve(); },
      reject(error) { clearTimeout(timeout); reject(error); },
    };
    try {
      connect();
    } catch (error) {
      fail({ code: "REMOTE_SOCKET", where: "remote-host.load", message: error instanceof Error ? error.message : String(error) });
    }
  });

  return {
    capabilities: { deliveryModes: ["push", "pull"], targetUiHz: UI_TARGET_HZ, sharedMemory: false, reconnect: true, publicCompanyReports: true, npcDecisionDiagnostics: capabilityRecord.npcDecisionDiagnostics },
    start(onUpdate, onFatalError) {
      if (disposed) throw new Error("远程会话已经销毁，不能重新启动");
      callback = onUpdate;
      fatalCallback = onFatalError ?? null;
      running = true;
      connect();
      if (cachedBaseline !== null) callback(cachedBaseline);
      void remoteJson(fetchFn, `${baseUrl}/api/running`, remotePost({ session_id: created.id, running: true }, token)).catch((error) => fail({ code: "REMOTE_RUNNING", where: "remote-host.start", message: error instanceof Error ? error.message : String(error) }));
    },
    stop() {
      running = false;
      void remoteJson(fetchFn, `${baseUrl}/api/running`, remotePost({ session_id: created.id, running: false }, token)).catch((error) => fail({ code: "REMOTE_RUNNING", where: "remote-host.stop", message: error instanceof Error ? error.message : String(error) }));
    },
    dispose() {
      if (disposed) return;
      const disposeFatalCallback = fatalCallback;
      disposed = true;
      running = false;
      socket?.close();
      socket = null;
      callback = null;
      fatalCallback = null;
      baselineWaiter?.reject(new Error("远程会话已销毁"));
      baselineWaiter = null;
      void remoteJson(fetchFn, `${baseUrl}/api/session?session_id=${encodeURIComponent(created.id)}`, {
        method: "DELETE",
        headers: { authorization: `Bearer ${token}` },
      }).catch((error) => {
        disposeFatalCallback?.({
          code: "REMOTE_DISPOSE",
          where: "remote-host.dispose",
          message: error instanceof Error ? error.message : String(error),
        });
      });
    },
    setSpeed(multiplier) {
      void remoteJson(fetchFn, `${baseUrl}/api/speed`, remotePost({ session_id: created.id, speed: remoteSpeedValue(multiplier) }, token)).catch((error) => fail({ code: "REMOTE_SPEED", where: "remote-host.setSpeed", message: error instanceof Error ? error.message : String(error) }));
    },
    async setPausePreferences(preferences: PausePreferences) {
      const currentGeneration = cachedBaseline?.generation ?? "1";
      await remoteJson(fetchFn, `${baseUrl}/api/pause-preferences`, remotePost(remotePausePreferencePayload(created.id, currentGeneration, preferences), token));
    },
    setFrameRate() {},
    async readSpeedMetrics() {
      return parseSpeedMetrics(await remoteJson(fetchFn, `${baseUrl}/api/speed?session_id=${encodeURIComponent(created.id)}`, { method: "GET" }));
    },
    getDeliveryMode() { return delivery; },
    setDeliveryMode(mode) {
      if (mode === delivery) return;
      delivery = mode;
      connectionGeneration += 1;
      socket?.close();
      socket = null;
      if (running) connect();
    },
    async submitIntent(intent: Intent) {
      const current = socket;
      if (current === null || current.readyState !== WebSocket.OPEN || awaitingBaseline) throw new Error("远程 Publisher 尚未完成权威基线同步，写请求未入队");
      const requestId = ++requestSequence;
      await new Promise<void>((resolve, reject) => {
        pendingCommands.set(requestId, { resolve, reject });
        current.send(JSON.stringify({ SubmitIntent: { request_id: requestId, intent } }));
      });
    },
    snapshot(): Snapshot {
      if (cachedBaseline === null) throw new Error("远程基线尚未就绪");
      return cachedBaseline.snapshot;
    },
    tick() {
      if (cachedBaseline === null) throw new Error("远程基线尚未就绪");
      return cachedBaseline.snapshot.tick;
    },
    day() {
      if (cachedBaseline === null) throw new Error("远程基线尚未就绪");
      return cachedBaseline.snapshot.day;
    },
    async save(candidate?: { readonly seq: number; readonly settledDate: string }): Promise<unknown> {
      if (disposed || cachedBaseline === null || awaitingBaseline) throw new Error("远程基线尚未就绪或已失效，不能保存");
      const queryGeneration = cachedBaseline.generation;
      const result = await remoteJson(fetchFn, `${baseUrl}/api/save`, remotePost(candidate === undefined
        ? { session_id: created.id, generation: queryGeneration }
        : { session_id: created.id, generation: queryGeneration, candidate }, token));
      if (disposed || cachedBaseline?.generation !== queryGeneration || awaitingBaseline) throw new Error("远程存档响应属于已过期会话 generation");
      return result;
    },
    async refreshBaseline(): Promise<void> {
      await requestResync();
      if (cachedBaseline === null) throw new Error("远程基线重同步未返回权威基线");
    },
    async playerWorkingOrders(): Promise<readonly PlayerWorkingOrder[]> {
      const queryBaseline = cachedBaseline;
      if (queryBaseline === null) throw new Error("远程基线尚未就绪，不能查询玩家活动委托");
      const queryGeneration = queryBaseline.generation;
      const queryEpoch = baselineEpoch;
      const params = new URLSearchParams({ session_id: created.id, generation: queryGeneration });
      const raw = await remoteJson(fetchFn, `${baseUrl}/api/player-working-orders?${params}`, {
        method: "GET",
        headers: { authorization: `Bearer ${token}` },
      });
      if (raw === null || typeof raw !== "object" || Array.isArray(raw)) throw new Error("远程玩家活动委托响应必须是对象");
      const response = raw as Record<string, unknown>;
      if (Object.keys(response).length !== 2 || typeof response.generation !== "string" || !Object.hasOwn(response, "orders")) throw new Error("远程玩家活动委托响应字段无效");
      if (response.generation !== queryGeneration || cachedBaseline?.generation !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("远程玩家活动委托响应属于已过期会话 generation");
      return normalizePlayerWorkingOrders(response.orders);
    },
    async calculateIndicators(input: IndicatorInput): Promise<IndicatorResults> {
      const normalized = normalizeIndicatorInput(input);
      const result = await remoteJson(fetchFn, `${baseUrl}/api/indicators`, remotePost({ session_id: created.id, ...normalized }, token));
      return normalizeIndicatorResults(result, normalized);
    },
    async load(slot: unknown) {
      if (disposed) throw new Error("远程会话已经销毁，不能读档");
      await remoteJson(fetchFn, `${baseUrl}/api/load`, remotePost({ session_id: created.id, slot }, token));
      if (disposed) throw new Error("远程会话已销毁，读档响应已失效");
      reportQueryEpoch += 1n;
      reportCompanies.clear();
      if (socket === null) await connectRestoredBaseline();
      else await requestResync();
    },
    async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
      const queryEpoch = reportQueryEpoch;
      const params = new URLSearchParams({ session_id: created.id });
      if (query.cursor !== null) params.set("cursor", query.cursor);
      if (query.page_size !== null) params.set("limit", String(query.page_size));
      const page = normalizePublicReportPage(await remoteJson(fetchFn, `${baseUrl}/api/companies/${encodeURIComponent(query.company_id)}/reports?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }));
      if (queryEpoch !== reportQueryEpoch) throw new Error("远程公开报告查询已因会话时间线变更失效，请重新查询");
      for (const report of page.reports) {
        const knownCompany = reportCompanies.get(report.id);
        if (report.company_id !== query.company_id || (knownCompany !== undefined && knownCompany !== query.company_id)) {
          throw new Error(`远程公开报告 ${report.id} 的公司 ${report.company_id} 与查询或已记录公司 ${query.company_id} 不匹配`);
        }
      }
      for (const report of page.reports) reportCompanies.set(report.id, report.company_id);
      return page;
    },
    async publicReportById(id: string): Promise<PublicReportSummary> {
      const queryEpoch = reportQueryEpoch;
      const companyId = reportCompanies.get(id);
      if (companyId === undefined) throw new Error(`远程公开报告 ${id} 的公司归属未知，请先查询该公司的公开报告页`);
      const params = new URLSearchParams({ session_id: created.id });
      const report = normalizePublicReportById(await remoteJson(fetchFn, `${baseUrl}/api/companies/${encodeURIComponent(companyId)}/reports/${encodeURIComponent(id)}?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }));
      if (queryEpoch !== reportQueryEpoch) throw new Error("远程公开报告查询已因会话时间线变更失效，请先查询新时间线的报告页");
      if (report.id !== id || report.company_id !== companyId) {
        throw new Error(`远程公开报告响应 ${report.company_id}/${report.id} 与请求 ${companyId}/${id} 不匹配`);
      }
      return report;
    },
    async npcDecisionTrace(account: number): Promise<readonly NpcDecisionTraceRecord[]> {
      if (!capabilityRecord.npcDecisionDiagnostics) throw new Error("远程服务端未协商启用 NPC 决策诊断");
      if (!Number.isSafeInteger(account) || account < 0) throw new Error("NPC 账户 ID 必须是非负安全整数");
      const queryBaseline = cachedBaseline;
      if (queryBaseline === null) throw new Error("远程基线尚未就绪，不能查询 NPC 决策诊断");
      const queryGeneration = queryBaseline.generation;
      const queryEpoch = baselineEpoch;
      const params = new URLSearchParams({ session_id: created.id, generation: queryGeneration });
      const result = await remoteJson(fetchFn, `${baseUrl}/api/diagnostics/npc/${account}?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } });
      if (result === null || typeof result !== "object" || Array.isArray(result)) throw new Error("远程 NPC 诊断响应必须是对象");
      const response = result as Record<string, unknown>;
      if (response.generation !== queryGeneration || cachedBaseline?.generation !== queryGeneration || baselineEpoch !== queryEpoch) throw new Error("远程 NPC 诊断响应属于已过期会话 generation");
      const diagnostics = parseNpcDecisionDiagnostics(response.diagnostics);
      if (diagnostics.kind === "unsupported") throw new Error("远程服务端未提供 NPC 决策诊断数据");
      return diagnostics.records;
    },
  };
}

function assertNever(value: never): never {
  throw new Error(`未处理远程消息：${String(value)}`);
}
