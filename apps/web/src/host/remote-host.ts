import { runObserverResync } from "./observer-resync.ts";
import { cancelInterval, cancelTimeout, isWebSocketOpen, openWebSocket, scheduleInterval, scheduleTimeout, type IntervalHandle, type TimeoutHandle } from "../../../../packages/libos/browser/browser-transport.ts";
import { parseCredentialToken } from "./remote-auth.ts";
import { checkRightsSubscriptionFailure, createRightsSubscription } from "./rights-subscription.ts";
import { normalizeMarketHistoryRequest, normalizeMarketHistoryPage, wireMarketHistoryRequest } from "./market-history.ts";
import { normalizeCurrentMinuteHistoryRequest, normalizeCurrentMinuteHistoryResponse, wireCurrentMinuteHistoryRequest } from "./current-minute-history.ts";
import { normalizePersonalTradeHistoryRequest, normalizePersonalTradeHistoryPage, wirePersonalTradeHistoryRequest } from "./personal-trade-history.ts";
import { normalizeConfirmationCursor, normalizePersonalTradeConfirmations, normalizeIntradayAverageInput, normalizeIntradayAverageCurveInput, normalizeIntradayAverageCurveResult } from "./intraday-average.ts";
import type { HistoricalStockData, PublicReportPage, PublicReportQuery, PublicReportSummary, SessionSetup, StockCode } from "../types/engine.ts";
import type { CodedIntent, CodedSnapshot } from "./protocol/stock-identity.ts";
import { wireIntentOf } from "./protocol/stock-identity.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import type { DeliveryMode, EngineHost } from "./engine-host.ts";
import { createCompanyReadQueries } from "./company-read.ts";
import { createCompanyWriteCommands, type CompanyWriteCommands } from "./company-write.ts";
import { companyWriteHttpJson } from "./company-write-http.ts";
import { parseRemoteMarketContext, type RemoteMarketContext } from "./remote-market-context.ts";
import { parsePlayerCompanyProposal } from "./player-proposals.ts";
import { parseCompanyReportCorrection } from "./report-corrections.ts";
import { string } from "../save/schema/primitives.ts";
import type { HostFailure, HostUpdate } from "./host-update.ts";
import { UI_TARGET_HZ } from "./host-update.ts";
import { remoteBaseUrl, remoteJson, remotePausePreferencePayload, remotePost, remoteSession, remoteWsUrl, remoteWsProtocols } from "./remote-request.ts";
import { RemoteRequestScope } from "./remote-request-scope.ts";
import { parseRemoteMessage, type RemoteMessage } from "./remote-wire.ts";
import { ReportQueryContext } from "./report-query-context.ts";
import { RemoteCommandRegistry } from "./remote-command-registry.ts";
import { RemotePublisherState } from "./remote-publisher-state.ts";
import { isReportCorrectionPrepareRejection } from "./protocol-failure.ts";
import { normalizePublicReportById, normalizePublicReportPage } from "./serde-normalize.ts";
import { assertValidSpeedMultiplier, parseSpeedMetrics } from "./speed.ts";
import { normalizePlayerWorkingOrders, type PlayerWorkingOrder } from "./player-working-orders.ts";
import { parseNpcDecisionDiagnostics, type NpcDecisionTraceRecord } from "./npc-decision-trace.ts";
import type { IndicatorInput, IndicatorResults } from "../components/indicator-results.ts";
import { normalizeIndicatorInput, normalizeIndicatorResults } from "./indicator-transport.ts";
import { parseHistoricalStockData } from "./stock-history.ts";
import { parseInitialAllocation } from "./initial-allocation.ts";
import { exact, record } from "./protocol/guards.ts";

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
  return connectRemoteHost(baseUrl, token, created.id, options, null);
}

export type RemoteMarketHost = EngineHost & {
  subscribeUpdates(onUpdate: Parameters<EngineHost["start"]>[0], onFatalError?: Parameters<EngineHost["start"]>[1]): Promise<void>;
};

export function attachRemoteHost(baseUrl: string, token: string, market: RemoteMarketContext, options: Pick<RemoteHostOptions, "fetchFn" | "webSocketFactory"> = {}): Promise<RemoteMarketHost> {
  return connectRemoteHost(remoteBaseUrl(baseUrl), parseCredentialToken(token), market.session_id, options, market);
}

async function connectRemoteHost(baseUrl: string, token: string, sessionId: string, options: RemoteHostOptions, market: RemoteMarketContext | null): Promise<RemoteMarketHost> {
  const fetchFn = options.fetchFn ?? fetch;
  const capabilityParams = new URLSearchParams({ session_id: sessionId });
  const rawCapabilities = await remoteJson(fetchFn, `${baseUrl}/api/host-capabilities?${capabilityParams}`, {
    method: "GET",
    headers: { authorization: `Bearer ${token}` },
  });
  if (rawCapabilities === null || typeof rawCapabilities !== "object" || Array.isArray(rawCapabilities)) throw new Error("远程宿主能力响应必须是对象");
  const capabilityRecord = rawCapabilities as Record<string, unknown>;
  if (Object.keys(capabilityRecord).length !== 1 || typeof capabilityRecord.npcDecisionDiagnostics !== "boolean") throw new Error("远程宿主能力响应不符合契约");
  const socketFactory = options.webSocketFactory ?? openWebSocket;
  const publisher = new RemotePublisherState();
  const reports = new ReportQueryContext();
  const commands = new RemoteCommandRegistry();
  let callback: ((update: HostUpdate) => void | boolean) | null = null;
  let pendingRestore: { readonly generation: string | null; readonly notify: () => void; awaitingAuthority: boolean } | null = null;
  let fatalCallback: ((failure: HostFailure) => void) | null = null;
  let disposed = false;
  let delivery: DeliveryMode = "push";
  let baselineDelivered = false;
  let companyWriteEpoch = 0;
  const requireCatalogResolver = () => { const baseline = publisher.baselineForRead(); if (baseline === null) throw new Error("远程只读查询需要baseline"); return baseline.catalogResolver; };
  type ObservationReply = Extract<RemoteMessage, { kind: "observer-cut" | "observer-fenced" }>;
  type Cut = Extract<ObservationReply, { kind: "observer-cut" }>;
  let fencingSupported = false;
  let lastRemoteCursor: { generation: string; tick: number; seq: number } | null = null;
  let fencingWaiter: { receive(reply: ObservationReply): void; reject(error: Error): void } | null = null;
  let resyncPromise: Promise<void> | null = null;
  let pullTimer: IntervalHandle | null = null;
  let pullResponseTimer: TimeoutHandle | null = null;
  let pullWaiting = false;
  let connectionTimer: TimeoutHandle | null = null;
  let reconnectTimer: TimeoutHandle | null = null;
  let reconnectAttempts = 0;
  const requests = new RemoteRequestScope();
  const requestJson = (url: string, init: RequestInit, decoder?: typeof remoteJson) => {
    if (disposed) return Promise.reject(new Error("远程会话已经销毁，请求未发送"));
    return requests.request(fetchFn, url, init, decoder);
  };
  const hostQuery = async (kind: string, data: unknown): Promise<unknown> => {
    const cursor = publisher.captureQueryCursor("远程基线尚未就绪，不能查询");
    const response = record(await requestJson(`${baseUrl}/api/host-query`, remotePost({ session_id: sessionId, generation: cursor.generation, query: { kind, data } }, token)), "远程查询响应");
    exact(response, ["generation", "value"], "远程查询响应");
    if (disposed) throw new Error("远程查询响应属于已销毁会话");
    publisher.assertQueryCursor(cursor, response.generation, "远程查询响应属于已过期 baseline/generation");
    return response.value;
  };
  const wireAverage = (sample: { turnoverCents: string; tradeCount: number; volumeShares: number }) => ({ turnover_cents: sample.turnoverCents, trade_count: sample.tradeCount, volume_shares: sample.volumeShares });
  const clearTransportTimers = () => {
    fencingWaiter?.reject(new Error("远程观察流已断开"));

    if (pullTimer !== null) cancelInterval(pullTimer);
    if (pullResponseTimer !== null) cancelTimeout(pullResponseTimer);
    if (connectionTimer !== null) cancelTimeout(connectionTimer);
    if (reconnectTimer !== null) cancelTimeout(reconnectTimer);
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
    companyWriteEpoch += 1;
    const baseline = publisher.baselineForRead();
    if (baseline !== null && publisher.isProtocolReady() && isWebSocketOpen(publisher.currentSocket())
      && isReportCorrectionPrepareRejection(failure, "server", baseline.generation)) {
      if (pullResponseTimer !== null) cancelTimeout(pullResponseTimer);
      pullResponseTimer = null;
      pullWaiting = false;
      interrupt("报表更正 prepare 被拒绝，旧提交已失效");
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

  function waitForObservationReply(socket: WebSocket, command: object): Promise<ObservationReply> {
    return new Promise((resolve, reject) => {
      if (fencingWaiter !== null) { reject(new Error("远程观察请求已在途")); return; }
      const timeout = scheduleTimeout(() => waiter.reject(new Error("远程观察请求超时（5000ms）")), 5000);
      const finish = () => { cancelTimeout(timeout); if (fencingWaiter === waiter) fencingWaiter = null; };
      const waiter = {
        receive(reply: ObservationReply) { finish(); resolve(reply); },
        reject(error: Error) { finish(); reject(error); },
      };
      fencingWaiter = waiter;
      try { socket.send(JSON.stringify(command)); }
      catch (error) { waiter.reject(error instanceof Error ? error : new Error(String(error))); }
    });
  }

  function requestFencedResync(socket: WebSocket): Promise<void> {
    let cutId: string | null = null;
    let lastCapture = 0n;
    return runObserverResync<Cut>({
      assertCurrent() {
        if (disposed || publisher.currentSocket() !== socket || !isWebSocketOpen(socket)) throw new Error("远程基线响应属于已关闭连接");
      },
      hold() {
        interrupt("远程基线重同步");
        publisher.beginResync();
      },
      floor: () => null, // Each server capture mints a new producer, with its own ordinal namespace.
      async capture() {
        const reply = await waitForObservationReply(socket, { CaptureObserver: {} });
        if (reply.kind !== "observer-cut" || BigInt(reply.captureId) <= lastCapture) throw new Error("远程观察切面缺少新的捕获身份");
        lastCapture = BigInt(reply.captureId);
        return { ordinal: reply.ordinal, value: reply };
      },
      canUse: ({ value: { update } }) => isCurrentRemoteBaseline(update),
      async fence({ ordinal, value: { captureId, update } }) {
        const reply = await waitForObservationReply(socket, { FenceObserver: { capture_id: captureId, timeline_generation: update.generation, publication_ordinal: ordinal } });
        if (reply.kind !== "observer-fenced" || reply.captureId !== captureId) throw new Error("远程观察栅栏回执不匹配");
        return reply.accepted;
      },
      install({ value: { captureId, update } }) {
        if (!installRemoteBaseline(update)) throw new Error("应用层拒绝权威基线");
        cutId = captureId;
      },
      release() {
        if (cutId === null) throw new Error("远程观察切面尚未安装");
        socket.send(JSON.stringify({ ReleaseObserver: { capture_id: cutId } }));
      },
    });
  }

  function isCurrentRemoteBaseline(update: Extract<HostUpdate, { type: "baseline" }>): boolean {
    const old = lastRemoteCursor;
    return old === null || BigInt(update.generation) > BigInt(old.generation)
      || (update.generation === old.generation && update.snapshot.tick >= old.tick && update.snapshot.seq >= old.seq);
  }

  function installRemoteBaseline(update: Extract<HostUpdate, { type: "baseline" }>): boolean {
    if (!isCurrentRemoteBaseline(update)) throw new Error("远程权威基线回卷已安装的 generation/tick/seq");
    publisher.installBaseline(update, () => reports.invalidate());
    if (pendingRestore !== null) {
      if (pendingRestore.generation !== null && update.generation !== pendingRestore.generation) pendingRestore.notify();
      if (pendingRestore.awaitingAuthority) pendingRestore = null;
    }
    if (!deliver(update)) return false;
    lastRemoteCursor = { generation: update.generation, tick: update.snapshot.tick, seq: update.snapshot.seq };
    baselineDelivered = callback !== null;
    publisher.resolveBaselineWaiter();
    return true;
  }

  function noteDeliveredProtocol(update: Extract<HostUpdate, { type: "protocol" }>): void {
    // Remote wire is JSON. Only inspect cursor fields to avoid decoding the batch twice.
    const variants = update.update as { TickBatch?: { frames?: Array<{ tick?: number; seq_to?: number }> }; CivilUpdate?: { tick?: number; seq_to?: number } };
    const cursor = variants.TickBatch?.frames?.at(-1) ?? variants.CivilUpdate;
    if (!cursor || typeof cursor.tick !== "number" || typeof cursor.seq_to !== "number") return;
    const old = lastRemoteCursor;
    if (old !== null && old.generation === update.generation && (cursor.tick < old.tick || cursor.seq_to < old.seq)) return;
    lastRemoteCursor = { generation: update.generation, tick: cursor.tick, seq: cursor.seq_to };
  }

  const requestResync = (): Promise<void> => {
    if (resyncPromise !== null) return resyncPromise;
    const current = publisher.currentSocket();
    if (current === null || !isWebSocketOpen(current)) {
      return Promise.reject(new Error("远程 Publisher 未连接，无法请求基线重同步"));
    }
    if (fencingSupported) {
      const pending = requestFencedResync(current).catch((error: unknown) => {
        // A rejected exchange can leave the server-side stream held. Close only
        // this observation socket; never stop or dispose the remote market.
        if (!disposed && publisher.currentSocket() === current) {
          fail({ code: "REMOTE_RESYNC", where: "remote-host.fence", message: error instanceof Error ? error.message : String(error) });
        }
        throw error;
      });
      resyncPromise = pending;
      const clear = () => { if (resyncPromise === pending) resyncPromise = null; };
      void pending.then(clear, clear);
      return pending;
    }
    interrupt("远程基线重同步");
    publisher.beginResync();
    const pending = new Promise<void>((resolve, reject) => {
      const timeout = scheduleTimeout(() => fail({ code: "REMOTE_BASELINE_TIMEOUT", where: "remote-host.resync", message: "权威基线重同步超时（5000ms）" }), 5000);
      publisher.beginBaselineWait({ resolve() { cancelTimeout(timeout); resolve(); }, reject(error) { cancelTimeout(timeout); reject(error); } });
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
      if (pullResponseTimer !== null) cancelTimeout(pullResponseTimer);
      pullResponseTimer = null;
      pullWaiting = false;
    }
    switch (parsed.kind) {
      case "baseline":
        if (connectionTimer !== null) cancelTimeout(connectionTimer);
        connectionTimer = null;
        reconnectAttempts = 0;
        fencingSupported = parsed.fencing;
        if (!installRemoteBaseline(parsed.update)) return;
        resyncPromise = null;
        if (delivery === "pull" && pullTimer === null) {
          pullTimer = scheduleInterval(() => {
            const current = publisher.currentSocket();
            if (!pullWaiting && current !== null && isWebSocketOpen(current) && publisher.isProtocolReady()) {
              pullWaiting = true;
              pullResponseTimer = scheduleTimeout(() => recover("远程 GetFrame 响应超时（5000ms）", identity), 5000);
              pullResponseTimer.unref?.();
              try { current.send(JSON.stringify({ GetFrame: {} })); }
              catch (error) { recover(error instanceof Error ? error.message : String(error), identity); }
            }
          }, 16);
          pullTimer.unref?.();
        }
        return;
      case "observer-cut":
      case "observer-fenced":
        fencingWaiter?.receive(parsed);
        return;
      case "protocol":
        if (!publisher.isProtocolReady()) return;
        if (!publisher.isProtocolDeliverable(parsed.update.generation)) {
          void requestResync().catch((error) => {
            if (!disposed && publisher.isCurrentConnection(identity)) fail({ code: "REMOTE_RESYNC", where: "remote-host.generation", message: error instanceof Error ? error.message : String(error) });
          });
          return;
        }
        if (deliver(parsed.update)) noteDeliveredProtocol(parsed.update);
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
    reconnectTimer = scheduleTimeout(() => {
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
    fencingSupported = false;
    publisher.beginResync();
    const next = socketFactory(remoteWsUrl(baseUrl, sessionId, delivery), remoteWsProtocols(token));
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
    connectionTimer = scheduleTimeout(() => recover("远程连接未在 5000ms 内返回权威基线", identity), 5000);
    connectionTimer.unref?.();
  };

  const connectRestoredBaseline = (): Promise<void> => new Promise((resolve, reject) => {
    publisher.beginResync();
    const timeout = scheduleTimeout(() => fail({ code: "REMOTE_BASELINE_TIMEOUT", where: "remote-host.load", message: "读取恢复后的远程权威基线超时（5000ms）" }), 5000);
    publisher.beginBaselineWait({
      resolve() { cancelTimeout(timeout); resolve(); },
      reject(error) { cancelTimeout(timeout); reject(error); },
    });
    try {
      connect();
    } catch (error) {
      fail({ code: "REMOTE_SOCKET", where: "remote-host.load", message: error instanceof Error ? error.message : String(error) });
    }
  });

  async function withCompanyWrite<T>(proposal: boolean, execute: (commands: CompanyWriteCommands) => Promise<T>): Promise<T> {
    const hasCurrentConnection = () => publisher.currentSocket()?.readyState === 1;
    if (disposed || !hasCurrentConnection() || !publisher.isProtocolReady()) throw new Error("远程公司写命令需要健康的当前连接与 baseline");
    const cursor = publisher.captureQueryCursor("远程公司写命令缺少当前 baseline");
    const writeEpoch = companyWriteEpoch;
    const catalog = requireCatalogResolver();
    const assertCurrent = (responseGeneration: unknown = cursor.generation) => {
      if (disposed || !hasCurrentConnection() || !publisher.isProtocolReady()) throw new Error("远程公司写命令确认已过期或会话已销毁；结果未知，请核对权威状态");
      if (writeEpoch !== companyWriteEpoch) throw new Error("远程公司写命令确认属于更正拒绝前的旧提交，已过期；结果未知，请核对权威状态");
      publisher.assertQueryCursor(cursor, responseGeneration, "远程公司写命令确认属于已过期 baseline/generation；结果未知，请核对权威状态");
    };
    async function settled<V>(action: () => Promise<V>): Promise<V> {
      try { const value = await action(); assertCurrent(); return value; }
      catch (error) {
        try { assertCurrent(); }
        catch (stale) { throw new Error(stale instanceof Error ? stale.message : "远程公司写命令确认已过期", { cause: error }); }
        throw error;
      }
    }
    const context = await settled(async () => parseRemoteMarketContext(await requestJson(`${baseUrl}/api/market/context?${new URLSearchParams({ session_id: sessionId })}`, { method: "GET", headers: { authorization: `Bearer ${token}` } })));
    assertCurrent(context.generation);
    if (context.session_id !== sessionId) throw new Error("远程公司写命令 context.session_id 与当前会话不一致");
    if (proposal && (context.member === null || context.needs_rejoin)) throw new Error("远程公司提案需要当前可信 member 本人身份，请先加入市场");
    if (!proposal && !context.can_control) throw new Error("当前可信 context 没有市场控制权限，不能发送报表更正命令");
    const owner = proposal ? context.member!.account_id : undefined;
    const commands = createCompanyWriteCommands(async command => {
      assertCurrent();
      const [path, payload, rejectedCode] = command.kind === "ProposePlayerCompanyAction"
        ? ["propose-player-company-action", { proposal: command.proposal }, "PLAYER_COMPANY_ACTION_REJECTED"] as const
        : command.kind === "EnqueueReportCorrection"
          ? ["enqueue-report-correction", { request: command.request }, "REPORT_CORRECTION_REJECTED"] as const
          : ["cancel-report-correction", { operation_id: command.operation_id }, "REPORT_CORRECTION_REJECTED"] as const;
      const raw = await settled(() => requestJson(`${baseUrl}/api/${path}`, remotePost({ session_id: sessionId, generation: cursor.generation, ...payload }, token), (fetcher, url, init) => companyWriteHttpJson(fetcher, url, init, rejectedCode)));
      const response = record(raw, `远程 ${path}`);
      exact(response, command.kind === "ProposePlayerCompanyAction" ? ["generation", "receipt"] : ["generation", "operation_id"], `远程 ${path}`);
      assertCurrent(response.generation);
      return command.kind === "ProposePlayerCompanyAction" ? { receipt: response.receipt } : { operation_id: response.operation_id };
    }, () => catalog, owner);
    return execute(commands);
  }

  const controlGeneration = (): string => {
    const baseline = publisher.baselineForRead();
    if (baseline !== null && publisher.isProtocolReady()) return baseline.generation;
    // 共享连接必须有当前授权基线；仅旧的直接建会话契约从 generation 1 启动。
    if (market !== null) throw new Error("远程权威 generation 尚未就绪，控制命令未发送");
    return "1";
  };
  const listen = (onUpdate: Parameters<EngineHost["start"]>[0], onFatalError?: Parameters<EngineHost["start"]>[1]) => {
    if (disposed) throw new Error("远程会话已经销毁，不能重新启动");
    callback = onUpdate;
    fatalCallback = onFatalError ?? null;
    connect();
    const baseline = publisher.baselineForRead();
    if (baseline !== null && !baselineDelivered && publisher.isProtocolReady()) {
      if (!deliver(baseline)) throw new Error("应用层拒绝远程初始基线，启动未确认");
      baselineDelivered = true;
    }
  };

  return {
    subscribeRightsOffering: createRightsSubscription(async (eventId, requestedShares) => {
      const connected = () => !disposed && publisher.currentSocket()?.readyState === 1 && publisher.isProtocolReady();
      if (!connected()) throw new Error("远程配股认购需要健康的当前连接与 baseline");
      const cursor = publisher.captureQueryCursor("远程配股认购缺少当前 baseline");
      const epoch = companyWriteEpoch;
      const assertCurrent = (responseGeneration: unknown = cursor.generation) => {
        if (!connected() || epoch !== companyWriteEpoch) throw new Error("远程配股认购确认已过期；结果未知，请核对权威状态");
        publisher.assertQueryCursor(cursor, responseGeneration, "远程配股认购确认已过期；结果未知，请核对权威状态");
      };
      try {
        const context = parseRemoteMarketContext(await requestJson(`${baseUrl}/api/market/context?${new URLSearchParams({ session_id: sessionId })}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }));
        assertCurrent(context.generation);
        if (context.session_id !== sessionId || context.member === null || context.needs_rejoin) throw new Error("配股认购需要当前可信 member 本人身份，请先加入市场");
        let raw: unknown;
        try {
          raw = await requestJson(`${baseUrl}/api/subscribe-rights-offering`, remotePost({ session_id: sessionId, generation: cursor.generation, event_id: eventId, requested_shares: requestedShares }, token), (fetcher, url, init) => companyWriteHttpJson(fetcher, url, init, "RIGHTS_SUBSCRIPTION_REJECTED"));
        } catch (error) {
          checkRightsSubscriptionFailure(error, eventId, context.member.account_id);
          throw error;
        }
        const response = record(raw, "远程配股认购");
        exact(response, ["generation", "receipt"], "远程配股认购");
        assertCurrent(response.generation);
        return response.receipt;
      } catch (error) {
        try { assertCurrent(); } catch (stale) { throw new Error("远程配股认购确认已过期；结果未知，请核对权威状态", { cause: new AggregateError([stale], "认购确认已过期", { cause: error }) }); }
        throw error;
      }
    }),
    async proposePlayerCompanyAction(input) {
      const proposal = structuredClone(parsePlayerCompanyProposal(input));
      return withCompanyWrite(true, commands => commands.proposePlayerCompanyAction(proposal));
    },
    async enqueueReportCorrection(input) {
      const request = parseCompanyReportCorrection(input);
      return withCompanyWrite(false, commands => commands.enqueueReportCorrection(request));
    },
    async cancelReportCorrection(input) {
      const operationId = string(input, "company_write.operation_id");
      return withCompanyWrite(false, commands => commands.cancelReportCorrection(operationId));
    },
    ...createCompanyReadQueries(async query => {
      if (disposed || !publisher.isProtocolReady()) throw new Error("远程公司查询需要健康的当前会话基线");
      const cursor = publisher.captureQueryCursor("远程公司查询需要当前基线");
      const response = record(await requestJson(`${baseUrl}/api/company-read-query`, remotePost({ session_id: sessionId, generation: cursor.generation, query }, token)), "远程 company_read_query");
      exact(response, ["generation", "owner", "value"], "远程 company_read_query");
      if (disposed || !publisher.isProtocolReady()) throw new Error("远程公司查询响应属于已销毁或已中断会话");
      publisher.assertQueryCursor(cursor, response.generation, "远程公司查询响应属于已过期会话 generation");
      return { owner: response.owner, value: response.value };
    }, requireCatalogResolver),
    capabilities: { deliveryModes: ["push", "pull"], targetUiHz: UI_TARGET_HZ, sharedMemory: false, reconnect: true, publicCompanyReports: true, personalTradeHistory: true, indicatorCapabilities: { intradayAverage: true, macd: true, priceKdj: true, candleKdj: true }, npcDecisionDiagnostics: capabilityRecord.npcDecisionDiagnostics },
    async subscribeUpdates(onUpdate, onFatalError) {
      if (disposed) throw new Error("远程会话已经销毁，不能订阅");
      if (publisher.baselineForRead() !== null) {
        listen(onUpdate, onFatalError);
        return;
      }
      callback = onUpdate;
      fatalCallback = onFatalError ?? null;
      await connectRestoredBaseline();
    },
    async start(onUpdate, onFatalError) {
      if (market !== null && publisher.baselineForRead() === null) await connectRestoredBaseline();
      listen(onUpdate, onFatalError);
      await requestJson(`${baseUrl}/api/running`, remotePost({ session_id: sessionId, generation: controlGeneration(), running: true }, token));
    },
    async stop() {
      await requestJson(`${baseUrl}/api/running`, remotePost({ session_id: sessionId, generation: controlGeneration(), running: false }, token));
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
      // 离开窗口只断开本客户端；不能销毁其他玩家正在使用的共享市场。
      if (market !== null) return;
      await requests.request(fetchFn, `${baseUrl}/api/session?session_id=${encodeURIComponent(sessionId)}`, {
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
      await requestJson(`${baseUrl}/api/speed`, remotePost({ session_id: sessionId, generation: controlGeneration(), speed: remoteSpeedValue(multiplier) }, token));
    },
    async setPausePreferences(preferences: PausePreferences) {
      const currentGeneration = controlGeneration();
      await requestJson(`${baseUrl}/api/pause-preferences`, remotePost(remotePausePreferencePayload(sessionId, currentGeneration, preferences), token));
    },
    async readSpeedMetrics() {
      return parseSpeedMetrics(await requestJson(`${baseUrl}/api/speed?session_id=${encodeURIComponent(sessionId)}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }));
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
    async submitIntent(intent: CodedIntent) {
      const current = publisher.currentSocket();
      if (current === null || !isWebSocketOpen(current) || publisher.isAwaitingBaseline()) throw new Error("远程 Publisher 尚未完成权威基线同步，写请求未入队");
      const baseline = publisher.baselineForRead();
      if (baseline === null) throw new Error("远程权威 generation 尚未就绪，写请求未入队");
      const generation = baseline.generation;
      const wiredIntent = wireIntentOf(intent, baseline.catalogResolver);
      const requestId = commands.next();
      await new Promise<void>((resolve, reject) => {
        const timeout = scheduleTimeout(() => commands.rejectGateway(requestId, { code: "REMOTE_COMMAND_TIMEOUT", where: "remote-host.submitIntent", message: "入队确认超时（5000ms），结果未知，请核对权威委托状态，勿重复提交" }, true), 5000);
        commands.register(requestId, { resolve() { cancelTimeout(timeout); resolve(); }, reject(error) { cancelTimeout(timeout); reject(error); } });
        // 请求层翻译：公共 Intent 的代码串 code 译为数值 StockId 后再提交（ADR 0041）。
        try { current.send(JSON.stringify({ SubmitIntent: { request_id: requestId, generation, intent: wiredIntent } })); }
        catch (error) { commands.rejectGateway(requestId, { code: "REMOTE_COMMAND_SEND", where: "remote-host.submitIntent", message: `写请求发送中断，结果未知：${error instanceof Error ? error.message : String(error)}` }); }
      });
      if (disposed || !publisher.hasGeneration(generation) || publisher.isAwaitingBaseline()) throw new Error("远程入队确认属于已过期会话 generation，请核对权威委托状态，勿重复提交");
    },
    snapshot(): CodedSnapshot {
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
        ? { session_id: sessionId, generation: queryGeneration }
        : { session_id: sessionId, generation: queryGeneration, candidate }, token));
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
      const params = new URLSearchParams({ session_id: sessionId, generation: queryGeneration });
      const raw = await requestJson(`${baseUrl}/api/player-working-orders?${params}`, {
        method: "GET",
        headers: { authorization: `Bearer ${token}` },
      });
      if (raw === null || typeof raw !== "object" || Array.isArray(raw)) throw new Error("远程玩家活动委托响应必须是对象");
      const response = raw as Record<string, unknown>;
      if (Object.keys(response).length !== 2 || typeof response.generation !== "string" || !Object.hasOwn(response, "orders")) throw new Error("远程玩家活动委托响应字段无效");
      publisher.assertQueryCursor(cursor, response.generation, "远程玩家活动委托响应属于已过期会话 generation");
      return normalizePlayerWorkingOrders(response.orders, (() => { const baseline = publisher.baselineForRead(); if (baseline === null) throw new Error("远程活动委托需要 baseline"); return baseline.catalogResolver; })());
    },
    async queryStockHistory(code: StockCode): Promise<HistoricalStockData> {
      const cursor = publisher.captureQueryCursor("远程基线尚未就绪，不能查询股票历史");
      // HTTP 查询参数 code 为数值 StockId（ADR 0041）。
      const params = new URLSearchParams({ session_id: sessionId, generation: cursor.generation, code: String(requireCatalogResolver().stockId(code)) });
      const raw = await requestJson(`${baseUrl}/api/stock-history?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } });
      if (raw === null || typeof raw !== "object" || Array.isArray(raw)) throw new Error("远程股票历史响应必须是对象");
      const response = raw as Record<string, unknown>;
      if (Object.keys(response).length !== 2 || typeof response.generation !== "string" || !Object.hasOwn(response, "data")) throw new Error("远程股票历史响应字段无效");
      publisher.assertQueryCursor(cursor, response.generation, "远程股票历史响应属于已过期会话 generation");
      return parseHistoricalStockData(response.data, code, requireCatalogResolver());
    },
    async initialAllocation() {
      if (disposed) throw new Error("远程会话已经销毁，不能查询初始分配");
      if (publisher.baselineForRead() === null) await connectRestoredBaseline();
      const cursor = publisher.captureQueryCursor("远程初始分配需要当前权威基线");
      const params = new URLSearchParams({ session_id: sessionId, generation: cursor.generation });
      const response = record(await requestJson(`${baseUrl}/api/initial-allocation?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }), "远程初始分配响应");
      exact(response, ["generation", "data"], "远程初始分配响应");
      if (disposed) throw new Error("远程初始分配响应属于已销毁会话");
      publisher.assertQueryCursor(cursor, response.generation, "远程初始分配响应属于已过期会话 generation");
      return parseInitialAllocation(response.data, requireCatalogResolver().stockCatalog.map(stock => stock.code), requireCatalogResolver());
    },
    async calculateIndicators(input: IndicatorInput): Promise<IndicatorResults> {
      const normalized = normalizeIndicatorInput(input);
      const result = await requestJson(`${baseUrl}/api/indicators`, remotePost({ session_id: sessionId, ...normalized }, token));
      return normalizeIndicatorResults(result, normalized);
    },
    async queryMarketHistory(request) {
      const normalized = normalizeMarketHistoryRequest(request), identity = requireCatalogResolver();
      return normalizeMarketHistoryPage(await hostQuery("MarketHistory", wireMarketHistoryRequest(normalized, identity)), normalized, identity);
    },
    async queryCurrentMinuteHistory(request) {
      const normalized = normalizeCurrentMinuteHistoryRequest(request), identity = requireCatalogResolver();
      return normalizeCurrentMinuteHistoryResponse(await hostQuery("CurrentMinuteHistory", wireCurrentMinuteHistoryRequest(normalized, identity)), normalized, identity);
    },
    async queryPersonalTradeHistory(request) {
      const normalized = normalizePersonalTradeHistoryRequest(request), identity = requireCatalogResolver();
      return normalizePersonalTradeHistoryPage(await hostQuery("PersonalTradeHistory", wirePersonalTradeHistoryRequest(normalized, identity)), normalized, identity);
    },
    async queryPersonalTradeConfirmations(beforeReceipt = null) {
      const identity = requireCatalogResolver();
      return normalizePersonalTradeConfirmations(await hostQuery("PersonalTradeConfirmations", { before_receipt: normalizeConfirmationCursor(beforeReceipt) }), identity);
    },
    async calculateIntradayAverage(input) {
      const sample = normalizeIntradayAverageInput(input);
      return normalizeIntradayAverageCurveResult([await hostQuery("IntradayAverage", wireAverage(sample))], 1, [sample])[0];
    },
    async calculateIntradayAverageCurve(input) {
      const normalized = normalizeIntradayAverageCurveInput(input);
      return normalizeIntradayAverageCurveResult(await hostQuery("IntradayAverageCurve", normalized.samples.map(wireAverage)), normalized.samples.length, normalized.samples);
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
        await requestJson(`${baseUrl}/api/load`, remotePost({ session_id: sessionId, slot }, token));
        if (disposed) throw new Error("远程会话已销毁，读档响应已失效");
        restore.notify();
        reports.invalidate();
        if (publisher.currentSocket() === null) await connectRestoredBaseline();
        else {
          // A pending refresh may have captured just before the restore commits.
          // Wait for it, then request a new cut if it still denotes the old timeline.
          if (resyncPromise !== null) await resyncPromise;
          if ((publisher.baselineForRead()?.generation ?? null) === restore.generation) await requestResync();
        }
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
      const params = new URLSearchParams({ session_id: sessionId });
      if (query.cursor !== null) params.set("cursor", String(query.cursor));
      if (query.page_size !== null) params.set("limit", String(query.page_size));
      const page = normalizePublicReportPage(await requestJson(`${baseUrl}/api/companies/${encodeURIComponent(String(query.company_id))}/reports?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }));
      reports.acceptPage(query.company_id, queryEpoch, page);
      return page;
    },
    async publicReportById(id: string): Promise<PublicReportSummary> {
      const queryEpoch = reports.captureEpoch();
      const companyId = reports.companyFor(id);
      const params = new URLSearchParams({ session_id: sessionId });
      const report = normalizePublicReportById(await requestJson(`${baseUrl}/api/companies/${encodeURIComponent(String(companyId))}/reports/${encodeURIComponent(id)}?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }));
      reports.validateReport(queryEpoch, companyId, id, report);
      return report;
    },
    async npcDecisionTrace(account: number): Promise<readonly NpcDecisionTraceRecord[]> {
      if (!capabilityRecord.npcDecisionDiagnostics) throw new Error("远程服务端未协商启用 NPC 决策诊断");
      if (!Number.isSafeInteger(account) || account < 0) throw new Error("NPC 账户 ID 必须是非负安全整数");
      const cursor = publisher.captureQueryCursor("远程基线尚未就绪，不能查询 NPC 决策诊断");
      const queryGeneration = cursor.generation;
      const params = new URLSearchParams({ session_id: sessionId, generation: queryGeneration });
      const result = await requestJson(`${baseUrl}/api/diagnostics/npc/${account}?${params}`, { method: "GET", headers: { authorization: `Bearer ${token}` } });
      if (result === null || typeof result !== "object" || Array.isArray(result)) throw new Error("远程 NPC 诊断响应必须是对象");
      const response = result as Record<string, unknown>;
      publisher.assertQueryCursor(cursor, response.generation, "远程 NPC 诊断响应属于已过期会话 generation");
      const diagnostics = parseNpcDecisionDiagnostics(response.diagnostics, requireCatalogResolver());
      if (diagnostics.kind === "unsupported") throw new Error("远程服务端未提供 NPC 决策诊断数据");
      return diagnostics.records;
    },
  };
}


function assertNever(value: never): never {
  throw new Error(`未处理远程消息：${String(value)}`);
}
