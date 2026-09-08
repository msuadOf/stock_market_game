import type { EngineEvent, Intent, SaveSlot, SessionSetup, Snapshot } from "../types/engine";
import type { EngineHost } from "./engine-host";
import { requiresRuntimeSnapshot } from "./runtime-snapshot-policy.ts";

interface ApiErrorEnvelope {
  code: string;
  message: string;
}

type RemoteMessage =
  | { kind: "snapshot"; snapshot: Snapshot }
  | { kind: "event"; event: EngineEvent }
  | { kind: "resync"; missed: number };

interface RemoteHostOptions {
  baseUrl?: string;
  token?: string;
  fetchFn?: typeof fetch;
  webSocketFactory?: (url: string) => WebSocket;
}

const EVENT_NAMES = new Set([
  "Trade",
  "PriceTick",
  "AuctionTick",
  "AuctionCompleted",
  "DayBoundary",
  "IntentRejected",
  "SettlementError",
  "VError",
  "OrderCanceled",
  "OrderAccepted",
]);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isSnapshot(value: unknown): value is Snapshot {
  return isRecord(value)
    && Number.isSafeInteger(value.seq)
    && Number.isSafeInteger(value.tick)
    && Number.isSafeInteger(value.day)
    && isRecord(value.markets)
    && isRecord(value.accounts);
}

export function parseRemoteMessage(raw: string): RemoteMessage {
  let value: unknown;
  try {
    value = JSON.parse(raw);
  } catch (error) {
    throw new Error(`远程消息不是合法 JSON：${String(error)}`);
  }
  if (isSnapshot(value)) return { kind: "snapshot", snapshot: value };
  if (!isRecord(value)) throw new Error("远程消息必须是对象");
  const resync = value.ResyncRequired;
  if (isRecord(resync) && Number.isSafeInteger(resync.missed) && Number(resync.missed) >= 0) {
    return { kind: "resync", missed: Number(resync.missed) };
  }
  const entries = Object.entries(value);
  if (entries.length !== 1 || !EVENT_NAMES.has(entries[0]![0]) || !isRecord(entries[0]![1])) {
    throw new Error("远程消息不是已知的 Snapshot、Event 或 ResyncRequired");
  }
  const payload = entries[0]![1];
  if (!Number.isSafeInteger(payload.seq) || Number(payload.seq) < 0) {
    throw new Error("远程事件缺少合法的安全整数 seq");
  }
  return { kind: "event", event: value as EngineEvent };
}

export function remoteEventSeq(event: EngineEvent): number {
  const payload = Object.values(event)[0];
  if (!payload || !Number.isSafeInteger(payload.seq)) {
    throw new Error("远程事件缺少合法的安全整数 seq");
  }
  return payload.seq;
}

function normalizeBaseUrl(baseUrl: string): string {
  const trimmed = baseUrl.trim().replace(/\/+$/, "");
  if (!/^https?:\/\//i.test(trimmed)) {
    throw new Error(`远程服务地址必须以 http:// 或 https:// 开头：${baseUrl}`);
  }
  return trimmed;
}

function wsUrl(baseUrl: string, sessionId: string, token: string): string {
  const url = new URL(`${baseUrl}/ws`);
  url.protocol = url.protocol === "https:" ? "wss:" : "ws:";
  url.searchParams.set("session_id", sessionId);
  url.searchParams.set("token", token);
  return url.toString();
}

async function readApiError(response: Response): Promise<string> {
  const text = await response.text();
  try {
    const body = JSON.parse(text) as unknown;
    if (isRecord(body) && typeof body.code === "string" && typeof body.message === "string") {
      const error = body as unknown as ApiErrorEnvelope;
      return `${error.code}: ${error.message}`;
    }
  } catch {
    // 下方会保留原始响应文本；JSON 解析失败本身不是业务错误。
  }
  return text || response.statusText || "empty response";
}

async function requestJson<T>(
  fetchFn: typeof fetch,
  baseUrl: string,
  path: string,
  init?: RequestInit,
): Promise<T> {
  const response = await fetchFn(`${baseUrl}${path}`, init);
  if (!response.ok) {
    throw new Error(`远程服务请求失败（HTTP ${response.status}）：${await readApiError(response)}`);
  }
  const text = await response.text();
  if (!text) return undefined as T;
  try {
    return JSON.parse(text) as T;
  } catch (error) {
    throw new Error(`远程服务成功响应不是合法 JSON：${String(error)}`);
  }
}

function jsonPost(body: unknown): RequestInit {
  return {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
  };
}

export function remoteSpeedValue(multiplier: number): number | "Fastest" {
  if (multiplier === Infinity) return "Fastest";
  if (!Number.isFinite(multiplier) || multiplier <= 0) {
    throw new Error(`非法速度倍率：${multiplier}（必须为正数或 Infinity）`);
  }
  return multiplier;
}

/** 创建连接 Axum 服务端的宿主；完成建会话和首帧快照后才返回。 */
export async function createRemoteHost(
  setup: SessionSetup,
  seed: bigint,
  options: RemoteHostOptions = {},
): Promise<EngineHost> {
  const env = (import.meta as ImportMeta & { env?: Record<string, string | undefined> }).env;
  const baseUrl = normalizeBaseUrl(options.baseUrl ?? env?.VITE_REMOTE_BASE_URL ?? "http://127.0.0.1:3000");
  const token = options.token ?? env?.VITE_REMOTE_TOKEN ?? "local-player";
  if (!token) throw new Error("远程会话 token 不能为空");
  const fetchFn = options.fetchFn ?? fetch;
  const socketFactory = options.webSocketFactory ?? ((url: string) => new WebSocket(url));

  const created = await requestJson<{ session_id: string }>(
    fetchFn,
    baseUrl,
    "/api/new",
    jsonPost({ setup, seed: seed.toString() }),
  );
  if (!created || typeof created.session_id !== "string" || !created.session_id) {
    throw new Error("远程服务没有返回合法 session_id");
  }
  const sessionId = created.session_id;
  let cachedSnapshot = await requestJson<Snapshot>(
    fetchFn,
    baseUrl,
    `/api/snapshot?session_id=${encodeURIComponent(sessionId)}`,
  );
  if (!isSnapshot(cachedSnapshot)) throw new Error("远程服务返回的首帧快照结构无效");

  let socket: WebSocket | null = null;
  let running = false;
  let disposed = false;
  let onEvents: ((events: EngineEvent[]) => void) | null = null;
  let onSnapshot: ((snapshot: Snapshot) => void) | null = null;
  let onFatalError: ((message: string) => void) | null = null;
  let lastSeq = cachedSnapshot.seq;
  let messageChain = Promise.resolve();
  let failureInProgress = false;

  const fail = (message: string) => {
    if (failureInProgress || disposed) return;
    failureInProgress = true;
    running = false;
    const failedSocket = socket;
    socket = null;
    failedSocket?.close();
    void requestJson<void>(
      fetchFn,
      baseUrl,
      "/api/running",
      jsonPost({ session_id: sessionId, running: false }),
    ).then(
      () => onFatalError?.(`${message}；远程会话已暂停`),
      (error) => onFatalError?.(`${message}；无法确认远程会话已暂停：${String(error)}`),
    );
  };

  const refreshSnapshot = async () => {
    const snapshot = await requestJson<Snapshot>(
      fetchFn,
      baseUrl,
      `/api/snapshot?session_id=${encodeURIComponent(sessionId)}`,
    );
    if (!isSnapshot(snapshot)) throw new Error("远程服务返回了无效快照");
    if (snapshot.seq < lastSeq) throw new Error("远程快照 seq 倒退");
    cachedSnapshot = snapshot;
    lastSeq = snapshot.seq;
    onSnapshot?.(snapshot);
  };

  const handleMessage = async (raw: string) => {
    const message = parseRemoteMessage(raw);
    if (message.kind === "snapshot") {
      if (message.snapshot.seq < lastSeq) return;
      cachedSnapshot = message.snapshot;
      lastSeq = message.snapshot.seq;
      onSnapshot?.(message.snapshot);
      return;
    }
    if (message.kind === "resync") {
      await refreshSnapshot();
      return;
    }
    const seq = remoteEventSeq(message.event);
    if (seq <= lastSeq) return;
    if (seq !== lastSeq + 1) {
      await refreshSnapshot();
      if (seq <= lastSeq) return;
      if (seq !== lastSeq + 1) {
        throw new Error(`事件序号不连续：本地 ${lastSeq}，收到 ${seq}`);
      }
    }
    lastSeq = seq;
    onEvents?.([message.event]);
    if (requiresRuntimeSnapshot([message.event])) await refreshSnapshot();
  };

  const connect = () => {
    if (socket || disposed) return;
    const next = socketFactory(wsUrl(baseUrl, sessionId, token));
    socket = next;
    next.onmessage = (event) => {
      messageChain = messageChain
        .then(() => handleMessage(String(event.data)))
        .catch((error) => fail(`远程事件协议失败：${String(error)}`));
    };
    next.onerror = () => fail("远程 WebSocket 连接发生错误");
    next.onclose = () => {
      if (socket === next) socket = null;
      if (running && !disposed) fail("远程 WebSocket 意外断开");
    };
  };

  const setRemoteRunning = (value: boolean) => {
    void requestJson<void>(
      fetchFn,
      baseUrl,
      "/api/running",
      jsonPost({ session_id: sessionId, running: value }),
    ).catch((error) => fail(`远程会话${value ? "恢复" : "暂停"}失败：${String(error)}`));
  };

  return {
    start(eventsCb, snapshotCb, fatalCb) {
      if (disposed) throw new Error("远程会话已经销毁，不能重新启动");
      onEvents = eventsCb;
      if (snapshotCb) onSnapshot = snapshotCb;
      if (fatalCb) onFatalError = fatalCb;
      running = true;
      connect();
      setRemoteRunning(true);
    },
    stop() {
      running = false;
      socket?.close();
      socket = null;
      setRemoteRunning(false);
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      running = false;
      socket?.close();
      socket = null;
      void requestJson<void>(
        fetchFn,
        baseUrl,
        `/api/session?session_id=${encodeURIComponent(sessionId)}`,
        { method: "DELETE" },
      ).catch((error) => console.error(`[RemoteHost] 释放会话失败：${String(error)}`));
      onEvents = null;
      onSnapshot = null;
      onFatalError = null;
    },
    setSpeed(multiplier) {
      void requestJson<void>(
        fetchFn,
        baseUrl,
        "/api/speed",
        jsonPost({ session_id: sessionId, speed: remoteSpeedValue(multiplier) }),
      ).catch((error) => fail(`设置远程倍速失败：${String(error)}`));
    },
    setFrameRate(_fps: number) {},
    submitIntent(intent: Intent) {
      void requestJson<void>(
        fetchFn,
        baseUrl,
        "/api/intent",
        jsonPost({ session_id: sessionId, intent }),
      ).catch((error) => fail(`提交远程意图失败：${String(error)}`));
    },
    snapshot() {
      return cachedSnapshot;
    },
    tick() {
      return cachedSnapshot.tick;
    },
    day() {
      return cachedSnapshot.day;
    },
    async save() {
      return await requestJson<SaveSlot>(
        fetchFn,
        baseUrl,
        "/api/save",
        jsonPost({ session_id: sessionId }),
      );
    },
    async load(slot: SaveSlot) {
      const snapshot = await requestJson<Snapshot>(
        fetchFn,
        baseUrl,
        "/api/load",
        jsonPost({ session_id: sessionId, slot }),
      );
      if (!isSnapshot(snapshot)) throw new Error("远程服务返回了无效的读档快照");
      cachedSnapshot = snapshot;
      lastSeq = snapshot.seq;
      onSnapshot?.(snapshot);
    },
  };
}
