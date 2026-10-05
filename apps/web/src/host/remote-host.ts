import type { HistoricalStockData, Intent, PublicReportAvailability, PublicReportAvailabilityQuery, PublicReportPage, PublicReportQuery, PublicReportSummary, SessionSetup, Snapshot, StockCode } from "../types/engine.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import { createTransportArchiveStore } from "../save/archive-store.ts";
import type { DeliveryMode, EngineHost, IndicatorCapabilities, IntradayAverageCurveInput, IntradayAverageInput, IntradayAverageResult, PersonalTradeConfirmation } from "./engine-host.ts";
import type { HostFailure, HostUpdate } from "./host-update.ts";
import { UI_TARGET_HZ } from "./host-update.ts";
import { remoteBaseUrl, remoteJson, remotePausePreferencePayload, remotePost, remoteWsUrl, remoteWsProtocols } from "./remote-request.ts";
import { parseRemoteMarketContext, type RemoteMarketContext } from "./remote-market-context.ts";
import { RemoteRequestScope } from "./remote-request-scope.ts";
import { parseRemoteMessage } from "./remote-wire.ts";
import { ReportQueryContext } from "./report-query-context.ts";
import { RemoteCommandRegistry } from "./remote-command-registry.ts";
import { RemotePublisherState } from "./remote-publisher-state.ts";
import { normalizePublicReportAvailability, normalizePublicReportAvailabilityQuery, normalizePublicReportById, normalizePublicReportPage } from "./serde-normalize.ts";
import { assertValidSpeedMultiplier, parseSpeedMetrics } from "./speed.ts";
import { normalizePlayerWorkingOrders, type PlayerWorkingOrder } from "./player-working-orders.ts";
import { parseNpcDecisionDiagnostics, type NpcDecisionTraceRecord } from "./npc-decision-trace.ts";
import type { IndicatorInput, IndicatorResults } from "../components/indicator-results.ts";
import { normalizeIndicatorInput, normalizeIndicatorResults } from "./indicator-transport.ts";
import { parseHistoricalStockData } from "./stock-history.ts";
import { parseInitialAllocation } from "./initial-allocation.ts";
import { normalizeConfirmationCursor, normalizeIntradayAverageCurveInput, normalizeIntradayAverageCurveResult, normalizeIntradayAverageInput, normalizeIntradayAverageResult, normalizePersonalTradeConfirmations } from "./intraday-average.ts";
import { normalizePersonalTradeHistoryRequest, normalizePersonalTradeHistoryPage } from "./personal-trade-history.ts";
import { normalizeMarketHistoryRequest, normalizeMarketHistoryPage, type MarketHistoryRequest, type MarketHistoryPage } from "./market-history.ts";
import { normalizeCurrentMinuteHistoryRequest, normalizeCurrentMinuteHistoryResponse, type CurrentMinuteHistoryRequest, type CurrentMinuteHistoryResponse } from "./current-minute-history.ts";
import type { PersonalTradeHistoryRequest, PersonalTradeHistoryPage } from "./engine-host.ts";
import { exact, record } from "./protocol/guards.ts";
import { parseReportCorrectionResponse, reportCorrectionFailureGeneration } from "./report-corrections.ts";
import { parseCompanyReportCorrection } from "../save/schema/company/report-corrections.ts";
import { parseMoney } from "../utils/money.ts";

type RemoteHostOptions = {
  readonly baseUrl?: string;
  readonly token?: string;
  readonly context?: RemoteMarketContext;
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
  const token = options.token;
  if (typeof token !== "string" || token.length === 0) throw new Error("远程会话缺少身份授权 token；请明确登录并选择市场");
  if (options.context === undefined) throw new Error("远程会话缺少已选择市场 context；不会自动创建市场");
  let marketContext = parseRemoteMarketContext(options.context);
  const created = { id: marketContext.session_id };
  setup = marketContext.setup;
  void seed;
  const contextSubscribers = new Set<(context: RemoteMarketContext) => void>();
  const capabilityParams = new URLSearchParams({ session_id: created.id });
  const rawCapabilities = await remoteJson(fetchFn, `${baseUrl}/api/host-capabilities?${capabilityParams}`, {
    method: "GET",
    headers: { authorization: `Bearer ${token}` },
  });
  if (rawCapabilities === null || typeof rawCapabilities !== "object" || Array.isArray(rawCapabilities)) throw new Error("远程宿主能力响应必须是对象");
  const capabilityRecord = rawCapabilities as Record<string, unknown>;
  const indicators = capabilityRecord.indicatorCapabilities;
  if (Object.keys(capabilityRecord).length !== 3 || typeof capabilityRecord.npcDecisionDiagnostics !== "boolean"
    || indicators === null || typeof indicators !== "object" || Array.isArray(indicators)
    || Object.keys(indicators as Record<string, unknown>).length !== 4
    || !["intradayAverage", "macd", "priceKdj", "candleKdj"].every((key) => typeof (indicators as Record<string, unknown>)[key] === "boolean")
    || typeof capabilityRecord.personalTradeHistory !== "boolean") throw new Error("远程宿主能力响应不符合契约");
  const indicatorCapabilities = indicators as IndicatorCapabilities;
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
  let baselineContextRequest = 0;
  const requests = new RemoteRequestScope();
  const requestJson = (url: string, init: RequestInit) => {
    if (disposed) return Promise.reject(new Error("远程会话已经销毁，请求未发送"));
    return requests.request(fetchFn, url, init);
  };
  const refreshContext = async () => {
    const value = await requestJson(`${baseUrl}/api/market/context?${new URLSearchParams({ session_id: created.id })}`, { method: "GET", headers: { authorization: `Bearer ${token}` } });
    const next = parseRemoteMarketContext(value);
    if (next.session_id !== created.id) throw new Error("市场 context 属于其他市场");
    return next;
  };
  const installContext = (next: RemoteMarketContext) => {
    marketContext = next;
    for (const subscriber of contextSubscribers) subscriber(next);
  };
  const controlGeneration = () => {
    if (!marketContext.can_control) throw new Error("当前身份没有市场控制授权");
    const baseline = publisher.baselineForRead();
    if (publisher.isAwaitingBaseline() && baseline !== null) throw new Error("市场基线正在同步，控制请求未发送");
    return baseline === null ? marketContext.generation : baseline.generation;
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
    if (failure.code === "REPORT_CORRECTION_REJECTED" && failure.recoverable === true) {
      if (!publisher.hasGeneration(reportCorrectionFailureGeneration(failure))) return;
      fatalCallback?.(failure);
      return;
    }
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
      case "baseline": {
        const contextRequest = ++baselineContextRequest;
        const accept = (next: RemoteMarketContext) => {
          if (disposed || !publisher.isCurrentConnection(identity) || contextRequest !== baselineContextRequest) return;
          if (next.generation !== parsed.update.generation) {
            void requestResync().catch((error) => {
              if (!disposed && publisher.isCurrentConnection(identity)) fail({ code: "REMOTE_CONTEXT_STALE", where: "remote-host.context", message: error instanceof Error ? error.message : String(error) });
            });
            return;
          }
          installContext(next);
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
        };
        if (marketContext.generation === parsed.update.generation) accept(marketContext);
        else {
          publisher.beginResync();
          void refreshContext().then((next) => {
            try { accept(next); }
            catch (error) {
              if (!disposed && publisher.isCurrentConnection(identity) && contextRequest === baselineContextRequest) fail({ code: "REMOTE_PROTOCOL", where: "remote-host.message", message: error instanceof Error ? error.message : String(error) });
            }
          }, (error) => {
            if (!disposed && publisher.isCurrentConnection(identity) && contextRequest === baselineContextRequest) fail({ code: "REMOTE_CONTEXT", where: "remote-host.context", message: error instanceof Error ? error.message : String(error) });
          });
        }
        return;
      }
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
        if (parsed.failure.context !== null && typeof parsed.failure.context === "object" && "generation" in parsed.failure.context
          && (parsed.failure.context as { generation: unknown }).generation !== marketContext.generation) return;
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
    get startupContext() { return { setup: marketContext.setup, seed: marketContext.seed, resumed: marketContext.resumed }; },
    marketContext() { return marketContext; },
    onMarketContext(subscriber) { contextSubscribers.add(subscriber); subscriber(marketContext); return () => contextSubscribers.delete(subscriber); },
    async rejoinMarket() {
      const generation = marketContext.generation;
      const next = parseRemoteMarketContext(await requestJson(`${baseUrl}/api/markets/join`, remotePost({ session_id: created.id, generation, confirmed_rejoin: true }, token)));
      if (disposed || marketContext.generation !== generation || next.generation !== generation) throw new Error("重新加入响应属于已过期市场");
      marketContext = next;
      await requestResync();
    },
    async resetMarket(nextSetup, nextSeed) {
      const generation = controlGeneration();
      await requestJson(`${baseUrl}/api/market/reset`, remotePost({ session_id: created.id, generation, setup: nextSetup, seed: nextSeed.toString() }, token));
      if (disposed) throw new Error("市场重置响应属于已断开连接");
      await requestResync();
    },
    async setRunning(running) {
      const generation = controlGeneration();
      await requestJson(`${baseUrl}/api/running`, remotePost({ session_id: created.id, generation, running }, token));
      if (disposed || marketContext.generation !== generation) throw new Error("市场控制响应属于已过期 generation");
    },
    async setAdmissionCash(cash) {
      const admissionCash = parseMoney(cash, "入场资金（分）");
      if (BigInt(admissionCash) < 0n) throw new Error("入场资金不能为负");
      const generation = controlGeneration();
      await requestJson(`${baseUrl}/api/market/admission-cash`, remotePost({ session_id: created.id, generation, admission_cash: admissionCash }, token));
      if (disposed || marketContext.generation !== generation) throw new Error("入场资金设置响应属于已过期 generation");
      const next = await refreshContext();
      if (next.generation !== generation || disposed) throw new Error("入场资金 context 属于已过期 generation");
      installContext(next);
    },
    archiveStore: createTransportArchiveStore(async (operation, slotId, name) => {
      const cursor = publisher.captureQueryCursor("存档操作需要当前远程会话基线");
      const params = new URLSearchParams({ session_id: created.id });
      if (slotId !== undefined) params.set("slot_id", slotId);
      let result: unknown;
      if (operation === "rename" || operation === "copy") {
        result = await requestJson(`${baseUrl}/api/archives/${operation}`, remotePost({ session_id: created.id, slot_id: slotId, name, generation: cursor.generation }, token));
      } else if (operation === "select") {
        result = await requestJson(`${baseUrl}/api/archives/select`, remotePost({ session_id: created.id, slot_id: slotId, generation: cursor.generation }, token));
      } else {
        if (operation === "delete") params.set("generation", cursor.generation);
        result = await requestJson(`${baseUrl}/api/archives${operation === "load" ? "/load" : ""}?${params}`, { method: operation === "delete" ? "DELETE" : "GET", headers: { authorization: `Bearer ${token}` } });
      }
      publisher.assertQueryCursor(cursor, cursor.generation, "存档响应属于已过期会话");
      return result;
    }),
    capabilities: { persistence: "remote", deliveryModes: ["push", "pull"], targetUiHz: UI_TARGET_HZ, sharedMemory: false, reconnect: true, publicCompanyReports: true, npcDecisionDiagnostics: capabilityRecord.npcDecisionDiagnostics, indicatorCapabilities, personalTradeHistory: capabilityRecord.personalTradeHistory },
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
    },
    async stop() {
      const generation = controlGeneration();
      await requestJson(`${baseUrl}/api/running`, remotePost({ session_id: created.id, generation, running: false }, token));
    },
    async dispose() {
      if (disposed) return;
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
      contextSubscribers.clear();
    },
    async setSpeed(multiplier) {
      const generation = controlGeneration();
      await requestJson(`${baseUrl}/api/speed`, remotePost({ session_id: created.id, generation, speed: remoteSpeedValue(multiplier) }, token));
      if (disposed || marketContext.generation !== generation) throw new Error("市场倍速响应属于已过期 generation");
    },
    async setPausePreferences(preferences: PausePreferences) {
      const currentGeneration = controlGeneration();
      await requestJson(`${baseUrl}/api/pause-preferences`, remotePost(remotePausePreferencePayload(created.id, currentGeneration, preferences), token));
    },
    setFrameRate() {},
    async readSpeedMetrics() {
      const generation = marketContext.generation;
      const metrics = parseSpeedMetrics(await requestJson(`${baseUrl}/api/speed?session_id=${encodeURIComponent(created.id)}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }));
      if (disposed || marketContext.generation !== generation) throw new Error("倍速指标响应属于已过期 generation");
      return metrics;
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
      if (marketContext.member === null) throw new Error("当前身份没有本市场资金账户；请本人明确重新加入后交易");
      const current = publisher.currentSocket();
      if (current === null || current.readyState !== WebSocket.OPEN || publisher.isAwaitingBaseline()) throw new Error("远程 Publisher 尚未完成权威基线同步，写请求未入队");
      const baseline = publisher.baselineForRead();
      if (baseline === null) throw new Error("远程权威 generation 尚未就绪，写请求未入队");
      const generation = baseline.generation;
      const requestId = commands.next();
      await new Promise<void>((resolve, reject) => {
        const timeout = setTimeout(() => commands.rejectGateway(requestId, { code: "REMOTE_COMMAND_TIMEOUT", where: "remote-host.submitIntent", message: "入队确认超时（5000ms），结果未知，请核对权威委托状态，勿重复提交" }, true), 5000);
        commands.register(requestId, { resolve() { clearTimeout(timeout); resolve(); }, reject(error) { clearTimeout(timeout); reject(error); } });
        try { current.send(JSON.stringify({ SubmitIntent: { request_id: requestId, generation, intent } })); }
        catch (error) { commands.rejectGateway(requestId, { code: "REMOTE_COMMAND_SEND", where: "remote-host.submitIntent", message: `写请求发送中断，结果未知：${error instanceof Error ? error.message : String(error)}` }); }
      });
      if (disposed || !publisher.hasGeneration(generation) || publisher.isAwaitingBaseline()) throw new Error("远程入队确认属于已过期会话 generation，请核对权威委托状态，勿重复提交");
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
      if (marketContext.member === null) return [];
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
    async queryStockHistory(code: StockCode): Promise<HistoricalStockData> {
      const cursor = publisher.captureQueryCursor("远程基线尚未就绪，不能查询股票历史");
      const params = new URLSearchParams({ session_id: created.id, generation: cursor.generation, code });
      const raw = await requestJson(`${baseUrl}/api/stock-history?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } });
      if (raw === null || typeof raw !== "object" || Array.isArray(raw)) throw new Error("远程股票历史响应必须是对象");
      const response = raw as Record<string, unknown>;
      if (Object.keys(response).length !== 2 || typeof response.generation !== "string" || !Object.hasOwn(response, "data")) throw new Error("远程股票历史响应字段无效");
      publisher.assertQueryCursor(cursor, response.generation, "远程股票历史响应属于已过期会话 generation");
      return parseHistoricalStockData(response.data, code);
    },
    async initialAllocation() {
      if (disposed) throw new Error("远程会话已经销毁，不能查询初始分配");
      if (publisher.baselineForRead() === null) await connectRestoredBaseline();
      const cursor = publisher.captureQueryCursor("远程初始分配需要当前权威基线");
      const params = new URLSearchParams({ session_id: created.id, generation: cursor.generation });
      const response = record(await requestJson(`${baseUrl}/api/initial-allocation?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }), "远程初始分配响应");
      exact(response, ["generation", "data"], "远程初始分配响应");
      if (disposed) throw new Error("远程初始分配响应属于已销毁会话");
      publisher.assertQueryCursor(cursor, response.generation, "远程初始分配响应属于已过期会话 generation");
      return parseInitialAllocation(response.data, setup.stocks.map((stock) => stock.code));
    },
    async calculateIndicators(input: IndicatorInput): Promise<IndicatorResults> {
      const normalized = normalizeIndicatorInput(input);
      const result = await requestJson(`${baseUrl}/api/indicators`, remotePost({ session_id: created.id, ...normalized }, token));
      return normalizeIndicatorResults(result, normalized);
    },
    async calculateIntradayAverage(input: IntradayAverageInput): Promise<IntradayAverageResult | null> {
      const normalized = normalizeIntradayAverageInput(input);
      const cursor = publisher.captureQueryCursor("远程 VWAP 查询需要当前会话 generation");
      const result = await requestJson(`${baseUrl}/api/intraday-average`, remotePost({ session_id: created.id, generation: cursor.generation, turnover_cents: normalized.turnoverCents, trade_count: normalized.tradeCount, volume_shares: normalized.volumeShares }, token));
      if (disposed || !publisher.hasGeneration(cursor.generation) || publisher.isAwaitingBaseline()) throw new Error("远程 VWAP 响应属于已过期会话 generation");
      return normalizeIntradayAverageResult(result);
    },
    async calculateIntradayAverageCurve(input: IntradayAverageCurveInput): Promise<readonly (IntradayAverageResult | null)[]> {
      const normalized = normalizeIntradayAverageCurveInput(input);
      const cursor = publisher.captureQueryCursor("远程 VWAP curve 查询需要当前会话 generation");
      const response = record(await requestJson(`${baseUrl}/api/intraday-average-curve`, remotePost({ session_id: created.id, generation: cursor.generation, series_key: normalized.seriesKey, samples: normalized.samples.map(({ turnoverCents, tradeCount, volumeShares }) => ({ turnover_cents: turnoverCents, trade_count: tradeCount, volume_shares: volumeShares })) }, token)), "远程 VWAP curve response");
      exact(response, ["generation", "series_key", "results"], "远程 VWAP curve response");
      publisher.assertQueryCursor(cursor, response.generation, "远程 VWAP curve 响应属于已过期会话 generation");
      if (response.series_key !== normalized.seriesKey) throw new Error("远程 VWAP curve 响应 series_key 与请求不匹配");
      return normalizeIntradayAverageCurveResult(response.results, normalized.samples.length, normalized.samples);
    },
    async queryPersonalTradeConfirmations(beforeReceipt: string | null = null): Promise<readonly PersonalTradeConfirmation[]> {
      if (marketContext.member === null) return [];
      const cursor = publisher.captureQueryCursor("远程交割单查询需要当前会话 generation");
      const params = new URLSearchParams({ session_id: created.id, generation: cursor.generation });
      const normalizedCursor = normalizeConfirmationCursor(beforeReceipt);
      if (normalizedCursor !== null) params.set("before_receipt", normalizedCursor);
      const response = record(await requestJson(`${baseUrl}/api/personal-trade-confirmations?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }), "远程本人交割单 response");
      exact(response, ["generation", "confirmations"], "远程本人交割单 response");
      publisher.assertQueryCursor(cursor, response.generation, "远程本人交割单响应属于已过期会话 generation");
      return normalizePersonalTradeConfirmations(response.confirmations);
    },
    async queryCurrentMinuteHistory(request: CurrentMinuteHistoryRequest): Promise<CurrentMinuteHistoryResponse> {
      const normalized = normalizeCurrentMinuteHistoryRequest(request), account = marketContext.member?.account_id ?? null;
      const cursor = publisher.captureQueryCursor("远程当前分钟需要当前generation");
      const response = record(await requestJson(`${baseUrl}/api/current-minute-history`, remotePost({ session_id: created.id, generation: cursor.generation, query: normalized }, token)), "远程当前分钟");
      exact(response, ["generation", "response"], "远程当前分钟");
      publisher.assertQueryCursor(cursor, response.generation, "远程当前分钟属于过期generation");
      if ((marketContext.member?.account_id ?? null) !== account) throw new Error("远程当前分钟响应属于已切换的本人身份");
      return normalizeCurrentMinuteHistoryResponse(response.response, normalized);
    },
    async queryMarketHistory(request: MarketHistoryRequest): Promise<MarketHistoryPage> {
      const normalized = normalizeMarketHistoryRequest(request);
      const account = marketContext.member?.account_id ?? null;
      const cursor = publisher.captureQueryCursor("远程量价历史需要当前generation");
      const response = record(await requestJson(`${baseUrl}/api/market-history`, remotePost({ session_id: created.id, generation: cursor.generation, query: normalized }, token)), "远程永久量价历史");
      exact(response, ["generation", "page"], "远程永久量价历史");
      publisher.assertQueryCursor(cursor, response.generation, "远程量价历史属于过期generation");
      if ((marketContext.member?.account_id ?? null) !== account) throw new Error("远程量价历史响应属于已切换的本人身份");
      return normalizeMarketHistoryPage(response.page, normalized);
    },
    async queryPersonalTradeHistory(request: PersonalTradeHistoryRequest): Promise<PersonalTradeHistoryPage> {
      if (marketContext.member === null) throw new Error("尚未入场，不能查询本人日期交割历史");
      const account = marketContext.member.account_id;
      const normalized = normalizePersonalTradeHistoryRequest(request);
      const cursor = publisher.captureQueryCursor("远程日期交割历史需要当前generation");
      const response = record(await requestJson(`${baseUrl}/api/personal-trade-history`, remotePost({ session_id: created.id, generation: cursor.generation, query: normalized }, token)), "远程本人日期历史");
      exact(response, ["generation", "page"], "远程本人日期历史");
      publisher.assertQueryCursor(cursor, response.generation, "远程日期交割历史属于过期generation");
      if (marketContext.member?.account_id !== account) throw new Error("远程日期交割历史响应属于已切换的本人账户");
      return normalizePersonalTradeHistoryPage(response.page, normalized);
    },
    async load(slot: unknown, archiveSlotId?: string, onRestored?: () => void) {
      if (disposed) throw new Error("远程会话已经销毁，不能读档");
      if (pendingRestore !== null) throw new Error("上一项远程读档尚未确认，请先刷新权威基线");
      const generation = controlGeneration();
      let notified = false;
      const restore = { generation, awaitingAuthority: false, notify() {
        if (notified || disposed) return;
        notified = true;
        onRestored?.();
      } };
      pendingRestore = restore;
      try {
        await requestJson(`${baseUrl}/api/load`, remotePost({ session_id: created.id, generation, slot, ...(archiveSlotId === undefined ? {} : { archive_slot_id: archiveSlotId }) }, token));
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
    async submitReportCorrection(request) {
      const normalized = parseCompanyReportCorrection(request);
      const cursor = publisher.captureQueryCursor("财报更正需要当前市场 generation");
      const response = record(await requestJson(`${baseUrl}/api/market/report-corrections`, remotePost({ session_id: created.id, generation: cursor.generation, request: normalized }, token)), "财报更正确认");
      if (disposed) throw new Error("财报更正确认属于已销毁市场");
      publisher.assertQueryCursor(cursor, response.generation, "财报更正确认属于已过期 generation");
      parseReportCorrectionResponse(response, cursor.generation, true);
    },
    async cancelReportCorrection(operationId) {
      if (operationId.trim().length === 0) throw new Error("更正 operation_id 必须非空");
      const cursor = publisher.captureQueryCursor("取消财报更正需要当前市场 generation");
      const init = remotePost({ session_id: created.id, generation: cursor.generation, operation_id: operationId }, token);
      const response = record(await requestJson(`${baseUrl}/api/market/report-corrections`, { ...init, method: "DELETE" }), "财报更正取消确认");
      if (disposed) throw new Error("取消确认属于已销毁市场");
      publisher.assertQueryCursor(cursor, response.generation, "取消财报更正确认属于已过期 generation");
      parseReportCorrectionResponse(response, cursor.generation, true);
    },
    async queryReportCorrections() {
      const cursor = publisher.captureQueryCursor("财报更正查询需要当前市场 generation");
      const params = new URLSearchParams({ session_id: created.id, generation: cursor.generation });
      const response = record(await requestJson(`${baseUrl}/api/market/report-corrections?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }), "财报更正查询");
      if (disposed) throw new Error("财报更正查询属于已销毁市场");
      publisher.assertQueryCursor(cursor, response.generation, "财报更正查询属于已过期 generation");
      return parseReportCorrectionResponse(response, cursor.generation, false);
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
    async queryPublicReportAvailability(query: PublicReportAvailabilityQuery): Promise<PublicReportAvailability> {
      const normalizedQuery = normalizePublicReportAvailabilityQuery(query);
      const queryEpoch = reports.captureEpoch();
      const params = new URLSearchParams({ session_id: created.id });
      const availability = await remoteJson(fetchFn, `${baseUrl}/api/companies/${encodeURIComponent(normalizedQuery.company_id)}/reports/availability?${params}`, {
        method: "POST",
        headers: { authorization: `Bearer ${token}`, "content-type": "application/json" },
        body: JSON.stringify(normalizedQuery),
      });
      reports.assertCurrent(queryEpoch, "请重新查询");
      return normalizePublicReportAvailability(availability, normalizedQuery);
    },
    async npcDecisionTrace(account: string): Promise<readonly NpcDecisionTraceRecord[]> {
      if (!capabilityRecord.npcDecisionDiagnostics) throw new Error("远程服务端未协商启用 NPC 决策诊断");
      if (typeof account !== "string" || !/^(0|[1-9]\d*)$/.test(account) || account.length > 20 || BigInt(account) > 18_446_744_073_709_551_615n) throw new Error("NPC 账户 ID 必须是规范 u64 非负十进制字符串");
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
