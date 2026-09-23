import type { Intent, PublicReportPage, PublicReportQuery, PublicReportSummary, SessionSetup, Snapshot } from "../types/engine.ts";
import type { PausePreferences } from "../types/generated/PausePreferences.ts";
import type { DeliveryMode, EngineHost } from "./engine-host.ts";
import type { HostFailure, HostUpdate } from "./host-update.ts";
import { UI_TARGET_HZ } from "./host-update.ts";
import { remoteBaseUrl, remoteJson, remotePausePreferencePayload, remotePost, remoteSession, remoteWsUrl } from "./remote-request.ts";
import { parseRemoteMessage } from "./remote-wire.ts";
import { normalizePublicReportPage } from "./serde-normalize.ts";
import { assertValidSpeedMultiplier, parseSpeedMetrics } from "./speed.ts";

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
  const environment = import.meta.env;
  const baseUrl = remoteBaseUrl(options.baseUrl ?? environment.VITE_REMOTE_BASE_URL ?? "http://127.0.0.1:3000");
  const fetchFn = options.fetchFn ?? fetch;
  const created = remoteSession(await remoteJson(fetchFn, `${baseUrl}/api/new`, remotePost({ setup, seed: seed.toString() })));
  const token = options.token ?? environment.VITE_REMOTE_TOKEN ?? created.token ?? "local-player";
  if (token === null || token.length === 0) throw new Error("远程会话缺少授权 token");
  const socketFactory = options.webSocketFactory ?? ((url: string) => new WebSocket(url));
  let socket: WebSocket | null = null;
  let callback: ((update: HostUpdate) => void) | null = null;
  let fatalCallback: ((failure: HostFailure) => void) | null = null;
  let cachedBaseline: Extract<HostUpdate, { type: "baseline" }> | null = null;
  let running = false;
  let disposed = false;
  let delivery: DeliveryMode = "push";
  let connectionGeneration = 0;
  let awaitingBaseline = true;
  let requestSequence = 0;
  const pendingCommands = new Map<number, { readonly resolve: () => void; readonly reject: (error: Error) => void }>();
  let baselineWaiter: { readonly resolve: () => void; readonly reject: (error: Error) => void } | null = null;

  const fail = (failure: HostFailure) => {
    running = false;
    socket?.close();
    socket = null;
    baselineWaiter?.reject(new Error(`${failure.code}: ${failure.message}`));
    baselineWaiter = null;
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
      if (running && !disposed && identity === connectionGeneration) {
        fail({ code: "REMOTE_SOCKET", where: "remote-host.socket", message: "远程 WebSocket 意外断开" });
      }
    };
  };

  return {
    capabilities: { deliveryModes: ["push", "pull"], targetUiHz: UI_TARGET_HZ, sharedMemory: false, reconnect: true, publicCompanyReports: false, npcDecisionDiagnostics: false },
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
      disposed = true;
      running = false;
      socket?.close();
      socket = null;
      callback = null;
      fatalCallback = null;
      baselineWaiter?.reject(new Error("远程会话已销毁"));
      baselineWaiter = null;
      void remoteJson(fetchFn, `${baseUrl}/api/session?session_id=${encodeURIComponent(created.id)}`, { method: "DELETE" }).catch(() => {});
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
    async save(): Promise<unknown> {
      return await remoteJson(fetchFn, `${baseUrl}/api/save`, remotePost({ session_id: created.id }, token));
    },
    async load(slot: unknown) {
      await remoteJson(fetchFn, `${baseUrl}/api/load`, remotePost({ session_id: created.id, slot }, token));
      await requestResync();
    },
    async queryPublicReports(query: PublicReportQuery): Promise<PublicReportPage> {
      return normalizePublicReportPage(await remoteJson(fetchFn, `${baseUrl}/api/public-reports/${encodeURIComponent(query.company_id)}?session_id=${encodeURIComponent(created.id)}`, { method: "GET", headers: { authorization: `Bearer ${token}` } }));
    },
    async publicReportById(id: string): Promise<PublicReportSummary> {
      throw new Error(`远程宿主未实现按 ID 查询公开报告：${id}`);
    },
  };
}

function assertNever(value: never): never {
  throw new Error(`未处理远程消息：${String(value)}`);
}
