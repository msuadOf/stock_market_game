import type { Intent, PublicReportPage, PublicReportQuery, PublicReportSummary, SessionSetup, Snapshot } from "../types/engine.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import type { DeliveryMode, EngineHost } from "./engine-host.ts";
import type { HostFailure, HostUpdate } from "./host-update.ts";
import { UI_TARGET_HZ } from "./host-update.ts";
import { remoteBaseUrl, remoteJson, remotePausePreferencePayload, remotePost, remoteSession, remoteWsUrl, remoteWsProtocols } from "./remote-request.ts";
import { RemoteRequestScope } from "./remote-request-scope.ts";
import { parseRemoteMessage } from "./remote-wire.ts";
import { ReportQueryContext } from "./report-query-context.ts";
import { RemoteCommandRegistry } from "./remote-command-registry.ts";
import { RemotePublisherState } from "./remote-publisher-state.ts";
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
  readonly webSocketFactory?: (url: string, protocols: string[]) => WebSocket;
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
  const socketFactory = options.webSocketFactory ?? ((url: string, protocols: string[]) => new WebSocket(url, protocols));
  const publisher = new RemotePublisherState();
  const reports = new ReportQueryContext();
  const commands = new RemoteCommandRegistry();
  let callback: ((update: HostUpdate) => void | boolean) | null = null;
  let pendingRestore: { readonly generation: string | null; readonly notify: () => void; awaitingAuthority: boolean } | null = null;
  let fatalCallback: ((failure: HostFailure) => void) | null = null;
  let disposed = false;
  let delivery: DeliveryMode = "push";
  let baselineDelivered = false;
  let resyncPromise: Promise<void> | null = null;
  let pullTimer: ReturnType<typeof setInterval> | null = null;
  let pullResponseTimer: ReturnType<typeof setTimeout> | null = null;
  let pullWaiting = false;
  let connectionTimer: ReturnType<typeof setTimeout> | null = null;
  let reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  let reconnectAttempts = 0;
  const requests = new RemoteRequestScope();
  const requestJson = (url: string, init: RequestInit) => {
    if (disposed) return Promise.reject(new Error("远程会话已经销毁，请求未发送"));
    return requests.request(fetchFn, url, init);
  };
  const clearTransportTimers = () => {
    if (pullTimer !== null) clearInterval(pullTimer);
    if (pullResponseTimer !== null) clearTimeout(pullResponseTimer);
    if (connectionTimer !== null) clearTimeout(connectionTimer);
    if (reconnectTimer !== null) clearTimeout(reconnectTimer);
    pullTimer = null;
    pullResponseTimer = null;
    pullWaiting = false;
    connectionTimer = null;
    reconnectTimer = null;
  };
  const interrupt = (reason: string) => {
    const message = `${reason}，确认中断，结果未知；请同步权威状态核对，勿重复提交委托`;
    commands.rejectAll({ code: "REMOTE_CONFIRMATION_INTERRUPTED", where: "remote-host.connection", message }, true);
    requests.interrupt(message);
  };
  const deliver = (update: HostUpdate): boolean => {
    if (callback?.(update) !== false) return true;
    clearTransportTimers();
    publisher.invalidateConnection();
    publisher.beginResync();
    publisher.detachSocket()?.close();
    interrupt("应用层拒绝远程更新");
    publisher.rejectBaselineWaiter(new Error("应用层拒绝权威基线，确认中断，结果未知"));
    return false;
  };

  const fail = (failure: HostFailure) => {
    clearTransportTimers();
    publisher.invalidateConnection();
    const failedSocket = publisher.detachSocket();
    const waiter = publisher.takeBaselineWaiter();
    waiter?.reject(new Error(`${failure.code}: ${failure.message}`));
    failedSocket?.close();
    commands.rejectAll({ ...failure, message: `${failure.message}；未确认请求的结果未知，请核对权威状态` });
    requests.interrupt(`${failure.message}；未确认请求的结果未知，请核对权威状态`);
    fatalCallback?.(failure);
  };

  const requestResync = (): Promise<void> => {
    if (resyncPromise !== null) return resyncPromise;
    const current = publisher.currentSocket();
    if (current === null || current.readyState !== WebSocket.OPEN) {
      return Promise.reject(new Error("远程 Publisher 未连接，无法请求基线重同步"));
    }
    interrupt("远程基线重同步");
    publisher.beginResync();
    const pending = new Promise<void>((resolve, reject) => {
      const timeout = setTimeout(() => fail({ code: "REMOTE_BASELINE_TIMEOUT", where: "remote-host.resync", message: "权威基线重同步超时（5000ms）" }), 5000);
      publisher.beginBaselineWait({ resolve() { clearTimeout(timeout); resolve(); }, reject(error) { clearTimeout(timeout); reject(error); } });
    });
    resyncPromise = pending;
    void pending.then(() => { if (resyncPromise === pending) resyncPromise = null; }, () => { if (resyncPromise === pending) resyncPromise = null; });
    try { current.send(JSON.stringify({ Resync: {} })); }
    catch (error) { publisher.rejectBaselineWaiter(error instanceof Error ? error : new Error(String(error))); }
    return pending;
  };

  const handle = (raw: string, identity: number) => {
    if (!publisher.isCurrentConnection(identity) || disposed) return;
    const parsed = parseRemoteMessage(raw, publisher.baselineForRead()?.generation ?? null);
    if (parsed.kind === "empty" || parsed.kind === "protocol" || parsed.kind === "baseline" || parsed.kind === "resync") {
      if (pullResponseTimer !== null) clearTimeout(pullResponseTimer);
      pullResponseTimer = null;
      pullWaiting = false;
    }
    switch (parsed.kind) {
      case "baseline":
        if (connectionTimer !== null) clearTimeout(connectionTimer);
        connectionTimer = null;
        reconnectAttempts = 0;
        publisher.installBaseline(parsed.update, () => reports.invalidate());
        if (pendingRestore !== null) {
          if (pendingRestore.generation !== null && parsed.update.generation !== pendingRestore.generation) pendingRestore.notify();
          if (pendingRestore.awaitingAuthority) pendingRestore = null;
        }
        if (!deliver(parsed.update)) return;
        baselineDelivered = callback !== null;
        publisher.resolveBaselineWaiter();
        resyncPromise = null;
        if (delivery === "pull" && pullTimer === null) {
          pullTimer = setInterval(() => {
            const current = publisher.currentSocket();
            if (!pullWaiting && current !== null && current.readyState === WebSocket.OPEN && publisher.isProtocolReady()) {
              pullWaiting = true;
              pullResponseTimer = setTimeout(() => recover("远程 GetFrame 响应超时（5000ms）", identity), 5000);
              pullResponseTimer.unref?.();
              try { current.send(JSON.stringify({ GetFrame: {} })); }
              catch (error) { recover(error instanceof Error ? error.message : String(error), identity); }
            }
          }, 16);
          pullTimer.unref?.();
        }
        return;
      case "protocol":
        if (!publisher.isProtocolReady()) return;
        if (!publisher.isProtocolDeliverable(parsed.update.generation)) {
          void requestResync().catch((error) => {
            if (!disposed && publisher.isCurrentConnection(identity)) fail({ code: "REMOTE_RESYNC", where: "remote-host.generation", message: error instanceof Error ? error.message : String(error) });
          });
          return;
        }
        deliver(parsed.update);
        return;
      case "failure":
        fail(parsed.failure);
        return;
      case "resync":
        void requestResync().catch((error) => {
          if (!disposed && publisher.isCurrentConnection(identity)) fail({ code: "REMOTE_RESYNC", where: "remote-host.resync", message: `${parsed.message}；${error instanceof Error ? error.message : String(error)}` });
        });
        return;
      case "empty":
        return;
      case "queued": {
        commands.resolveQueued(parsed.requestId);
        return;
      }
      case "gateway-error": {
        if (parsed.requestId === null) {
          fail(parsed.failure);
          return;
        }
        commands.rejectGateway(parsed.requestId, parsed.failure);
        return;
      }
      default:
        return assertNever(parsed);
    }
  };

  const recover = (message: string, identity: number) => {
    if (disposed || !publisher.isCurrentConnection(identity)) return;
    clearTransportTimers();
    publisher.invalidateConnection();
    const previous = publisher.detachSocket();
    previous?.close();
    publisher.beginResync();
    interrupt("远程连接中断");
    publisher.rejectBaselineWaiter(new Error("远程连接中断，基线确认结果未知，请等待恢复后重试"));
    if (++reconnectAttempts > 3) {
      fail({ code: "REMOTE_SOCKET", where: "remote-host.reconnect", message: `远程连接恢复失败（3 次重试）：${message}` });
      return;
    }
    reconnectTimer = setTimeout(() => {
      reconnectTimer = null;
      try { connect(); }
      catch (error) { recover(error instanceof Error ? error.message : String(error), publisher.invalidateConnection()); }
    }, 100 * 2 ** (reconnectAttempts - 1));
    reconnectTimer.unref?.();
  };

  const connect = () => {
    if (publisher.currentSocket() !== null || disposed) return;
    const identity = publisher.invalidateConnection();
    commands.clearInterrupted();
    publisher.beginResync();
    const next = socketFactory(remoteWsUrl(baseUrl, created.id, delivery), remoteWsProtocols(token));
    publisher.attachSocket(next);
    next.onmessage = (event) => {
      if (disposed || !publisher.isCurrentConnection(identity)) return;
      try {
        handle(String(event.data), identity);
      } catch (error) {
        fail({ code: "REMOTE_PROTOCOL", where: "remote-host.message", message: error instanceof Error ? error.message : String(error) });
      }
    };
    next.onerror = () => recover("远程 WebSocket 连接发生错误", identity);
    next.onclose = () => {
      recover("远程 WebSocket 意外断开", identity);
    };
    connectionTimer = setTimeout(() => recover("远程连接未在 5000ms 内返回权威基线", identity), 5000);
    connectionTimer.unref?.();
  };

  const connectRestoredBaseline = (): Promise<void> => new Promise((resolve, reject) => {
    publisher.beginResync();
    const timeout = setTimeout(() => fail({ code: "REMOTE_BASELINE_TIMEOUT", where: "remote-host.load", message: "读取恢复后的远程权威基线超时（5000ms）" }), 5000);
    publisher.beginBaselineWait({
      resolve() { clearTimeout(timeout); resolve(); },
      reject(error) { clearTimeout(timeout); reject(error); },
    });
    try {
      connect();
    } catch (error) {
      fail({ code: "REMOTE_SOCKET", where: "remote-host.load", message: error instanceof Error ? error.message : String(error) });
    }
  });

  return {
    capabilities: { deliveryModes: ["push", "pull"], targetUiHz: UI_TARGET_HZ, sharedMemory: false, reconnect: true, publicCompanyReports: true, npcDecisionDiagnostics: capabilityRecord.npcDecisionDiagnostics },
    async start(onUpdate, onFatalError) {
      if (disposed) throw new Error("远程会话已经销毁，不能重新启动");
      callback = onUpdate;
      fatalCallback = onFatalError ?? null;
      connect();
      const baseline = publisher.baselineForRead();
      if (baseline !== null && !baselineDelivered && publisher.isProtocolReady()) {
        if (!deliver(baseline)) throw new Error("应用层拒绝远程初始基线，启动未确认");
        baselineDelivered = true;
      }
      await requestJson(`${baseUrl}/api/running`, remotePost({ session_id: created.id, running: true }, token));
    },
    async stop() {
      await requestJson(`${baseUrl}/api/running`, remotePost({ session_id: created.id, running: false }, token));
    },
    async dispose() {
      if (disposed) return;
      const disposeFatalCallback = fatalCallback;
      disposed = true;
      pendingRestore = null;
      clearTransportTimers();
      publisher.invalidateConnection();
      interrupt("远程会话已销毁");
      publisher.currentSocket()?.close();
      publisher.detachSocket();
      callback = null;
      fatalCallback = null;
      publisher.rejectBaselineWaiter(new Error("远程会话已销毁，基线确认结果未知"));
      await requests.request(fetchFn, `${baseUrl}/api/session?session_id=${encodeURIComponent(created.id)}`, {
        method: "DELETE",
        headers: { authorization: `Bearer ${token}` },
      }).catch((error) => {
        disposeFatalCallback?.({
          code: "REMOTE_DISPOSE",
          where: "remote-host.dispose",
          message: error instanceof Error ? error.message : String(error),
        });
        throw error;
      });
    },
    async setSpeed(multiplier) {
      await requestJson(`${baseUrl}/api/speed`, remotePost({ session_id: created.id, speed: remoteSpeedValue(multiplier) }, token));
    },
    async setPausePreferences(preferences: PausePreferences) {
      const currentGeneration = publisher.baselineForRead()?.generation ?? "1";
      await requestJson(`${baseUrl}/api/pause-preferences`, remotePost(remotePausePreferencePayload(created.id, currentGeneration, preferences), token));
    },
    setFrameRate() {},
    async readSpeedMetrics() {
      return parseSpeedMetrics(await requestJson(`${baseUrl}/api/speed?session_id=${encodeURIComponent(created.id)}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }));
    },
    getDeliveryMode() { return delivery; },
    setDeliveryMode(mode) {
      if (mode === delivery) return;
      delivery = mode;
      clearTransportTimers();
      interrupt("远程 delivery 模式切换");
      publisher.rejectBaselineWaiter(new Error("远程 delivery 模式切换，基线确认结果未知"));
      publisher.beginResync();
      publisher.invalidateConnection();
      publisher.currentSocket()?.close();
      publisher.detachSocket();
      if (callback !== null) connect();
    },
    async submitIntent(intent: Intent) {
      const current = publisher.currentSocket();
      if (current === null || current.readyState !== WebSocket.OPEN || publisher.isAwaitingBaseline()) throw new Error("远程 Publisher 尚未完成权威基线同步，写请求未入队");
      const requestId = commands.next();
      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => commands.rejectGateway(requestId, { code: "REMOTE_COMMAND_TIMEOUT", where: "remote-host.submitIntent", message: "入队确认超时（5000ms），结果未知，请核对权威委托状态，勿重复提交" }, true), 5000);
        commands.register(requestId, { resolve() { clearTimeout(timeout); resolve(); }, reject(error) { clearTimeout(timeout); reject(error); } });
        try { current.send(JSON.stringify({ SubmitIntent: { request_id: requestId, intent } })); }
        catch (error) { commands.rejectGateway(requestId, { code: "REMOTE_COMMAND_SEND", where: "remote-host.submitIntent", message: `写请求发送中断，结果未知：${error instanceof Error ? error.message : String(error)}` }); }
      });
    },
    snapshot(): Snapshot {
      const baseline = publisher.baselineForRead();
      if (baseline === null) throw new Error("远程基线尚未就绪");
      return baseline.snapshot;
    },
    tick() {
      const baseline = publisher.baselineForRead();
      if (baseline === null) throw new Error("远程基线尚未就绪");
      return baseline.snapshot.tick;
    },
    day() {
      const baseline = publisher.baselineForRead();
      if (baseline === null) throw new Error("远程基线尚未就绪");
      return baseline.snapshot.day;
    },
    async save(candidate?: { readonly seq: number; readonly settledDate: string }): Promise<unknown> {
      const baseline = publisher.baselineForRead();
      if (disposed || baseline === null || publisher.isAwaitingBaseline()) throw new Error("远程基线尚未就绪或已失效，不能保存");
      const queryGeneration = baseline.generation;
      const result = await requestJson(`${baseUrl}/api/save`, remotePost(candidate === undefined
        ? { session_id: created.id, generation: queryGeneration }
        : { session_id: created.id, generation: queryGeneration, candidate }, token));
      if (disposed || !publisher.hasGeneration(queryGeneration) || publisher.isAwaitingBaseline()) throw new Error("远程存档响应属于已过期会话 generation");
      return result;
    },
    async refreshBaseline(): Promise<void> {
      await requestResync();
      if (publisher.baselineForRead() === null) throw new Error("远程基线重同步未返回权威基线");
    },
    async playerWorkingOrders(): Promise<readonly PlayerWorkingOrder[]> {
      const cursor = publisher.captureQueryCursor("远程基线尚未就绪，不能查询玩家活动委托");
      const queryGeneration = cursor.generation;
      const params = new URLSearchParams({ session_id: created.id, generation: queryGeneration });
      const raw = await requestJson(`${baseUrl}/api/player-working-orders?${params}`, {
        method: "GET",
        headers: { authorization: `Bearer ${token}` },
      });
      if (raw === null || typeof raw !== "object" || Array.isArray(raw)) throw new Error("远程玩家活动委托响应必须是对象");
      const response = raw as Record<string, unknown>;
      if (Object.keys(response).length !== 2 || typeof response.generation !== "string" || !Object.hasOwn(response, "orders")) throw new Error("远程玩家活动委托响应字段无效");
      publisher.assertQueryCursor(cursor, response.generation, "远程玩家活动委托响应属于已过期会话 generation");
      return normalizePlayerWorkingOrders(response.orders);
    },
    async calculateIndicators(input: IndicatorInput): Promise<IndicatorResults> {
      const normalized = normalizeIndicatorInput(input);
      const result = await requestJson(`${baseUrl}/api/indicators`, remotePost({ session_id: created.id, ...normalized }, token));
      return normalizeIndicatorResults(result, normalized);
    },
    async load(slot: unknown, onRestored?: () => void) {
      if (disposed) throw new Error("远程会话已经销毁，不能读档");
      if (pendingRestore !== null) throw new Error("上一项远程读档尚未确认，请先刷新权威基线");
      let notified = false;
      const restore = { generation: publisher.baselineForRead()?.generation ?? null, awaitingAuthority: false, notify() {
        if (notified || disposed) return;
        notified = true;
        onRestored?.();
      } };
      pendingRestore = restore;
      try {
        await requestJson(`${baseUrl}/api/load`, remotePost({ session_id: created.id, slot }, token));
        if (disposed) throw new Error("远程会话已销毁，读档响应已失效");
        restore.notify();
        reports.invalidate();
        if (publisher.currentSocket() === null) await connectRestoredBaseline();
        else await requestResync();
      } catch (error) {
        // HTTP 失败不等于恢复未提交；由随后权威 baseline 确认新/旧 generation。
        restore.awaitingAuthority = true;
        throw error;
      } finally {
        if (pendingRestore === restore && !restore.awaitingAuthority) pendingRestore = null;
      }
    },
    async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
      const queryEpoch = reports.captureEpoch();
      const params = new URLSearchParams({ session_id: created.id });
      if (query.cursor !== null) params.set("cursor", query.cursor);
      if (query.page_size !== null) params.set("limit", String(query.page_size));
      const page = normalizePublicReportPage(await requestJson(`${baseUrl}/api/companies/${encodeURIComponent(query.company_id)}/reports?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }));
      reports.acceptPage(query.company_id, queryEpoch, page);
      return page;
    },
    async publicReportById(id: string): Promise<PublicReportSummary> {
      const queryEpoch = reports.captureEpoch();
      const companyId = reports.companyFor(id);
      const params = new URLSearchParams({ session_id: created.id });
      const report = normalizePublicReportById(await requestJson(`${baseUrl}/api/companies/${encodeURIComponent(companyId)}/reports/${encodeURIComponent(id)}?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }));
      reports.validateReport(queryEpoch, companyId, id, report);
      return report;
    },
    async npcDecisionTrace(account: number): Promise<readonly NpcDecisionTraceRecord[]> {
      if (!capabilityRecord.npcDecisionDiagnostics) throw new Error("远程服务端未协商启用 NPC 决策诊断");
      if (!Number.isSafeInteger(account) || account < 0) throw new Error("NPC 账户 ID 必须是非负安全整数");
      const cursor = publisher.captureQueryCursor("远程基线尚未就绪，不能查询 NPC 决策诊断");
      const queryGeneration = cursor.generation;
      const params = new URLSearchParams({ session_id: created.id, generation: queryGeneration });
      const result = await requestJson(`${baseUrl}/api/diagnostics/npc/${account}?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } });
      if (result === null || typeof result !== "object" || Array.isArray(result)) throw new Error("远程 NPC 诊断响应必须是对象");
      const response = result as Record<string, unknown>;
      publisher.assertQueryCursor(cursor, response.generation, "远程 NPC 诊断响应属于已过期会话 generation");
      const diagnostics = parseNpcDecisionDiagnostics(response.diagnostics);
      if (diagnostics.kind === "unsupported") throw new Error("远程服务端未提供 NPC 决策诊断数据");
      return diagnostics.records;
    },
  };
}

function assertNever(value: never): never {
  throw new Error(`未处理远程消息：${String(value)}`);
}
