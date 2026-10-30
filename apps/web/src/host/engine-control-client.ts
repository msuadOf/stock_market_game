import type { WasmLifecycleObservation } from "./wasm-lifecycle-observation.ts";
import type { WasmFinalRelease } from "./wasm-document-memory-owner.ts";
import { parseHostFailure } from "./protocol-failure.ts";
import { parseWasmSessionShutdown } from "./wasm-shutdown-report.ts";

export interface EngineBusinessPort {
  addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  removeEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  postMessage(message: unknown): void;
  start(): void;
  close(): void;
}

type OwnerMessage = Readonly<Record<string, unknown>>;
let nextInstanceId = 1;
function exact(value: Readonly<Record<string, unknown>>, keys: readonly string[], label: string): void {
  if (Object.keys(value).sort().join(",") !== [...keys].sort().join(",")) throw new Error(`${label} 字段无效`);
}

function positiveSafeInteger(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 1) throw new Error(`${label} 必须是正安全整数`);
  return value;
}

function lifecycleEvent(value: unknown): WasmLifecycleObservation {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("controlWorker lifecycle event 必须为对象");
  const event = value as OwnerMessage;
  if (typeof event.type !== "string") throw new Error("controlWorker lifecycle event 缺少 type");
  const counts = ["activeChildren", "manifestEntries", "pendingRecovery"];
  const identity = ["workerId", "slot", "gen"];
  const identityField = (key: string) => {
    const value = event[key];
    if (typeof value !== "number" || !Number.isInteger(value) || value < (key === "workerId" ? 1 : 0) || value > 0xffffffff) throw new Error(`lifecycle.${key} 必须是合法 u32`);
  };
  if (event.type === "memoryRegistered") {
    exact(event, ["type", "allocationId", "role"], "memoryRegistered");
    if (event.allocationId !== 1 || event.role !== "engine") throw new Error("memoryRegistered 身份无效");
  } else if (event.type === "allocationConfigured") {
    exact(event, ["type", "allocationId", "role", "workerId", "slot", "gen"], "allocationConfigured");
    if (event.allocationId !== 1 || !["engine", "ingress", "child"].includes(String(event.role))) throw new Error("allocationConfigured 身份无效");
    if (event.role === "child") for (const key of identity) identityField(key);
    else if (event.workerId !== null || event.slot !== null || event.gen !== null) throw new Error("allocationConfigured 非 child 身份字段必须为 null");
  } else if (event.type === "childSpawn") {
    exact(event, ["type", ...identity, "name", "runtimeName"], "childSpawn");
    for (const key of identity) identityField(key);
    if (typeof event.name !== "string" || event.name.length === 0) throw new Error("childSpawn.name 无效");
    if (typeof event.runtimeName !== "string" || event.runtimeName.length === 0) throw new Error("childSpawn.runtimeName 无效");
  } else if (event.type === "childExit") {
    exact(event, ["type", ...identity, ...counts], "childExit");
    for (const key of identity) identityField(key);
    for (const key of counts) if (!Number.isSafeInteger(event[key]) || Number(event[key]) < 0) throw new Error(`childExit.${key} 无效`);
  } else if (event.type === "quiescent") {
    exact(event, ["type", "role", ...counts], "quiescent");
    if (event.role !== "engine" && event.role !== "ingress") throw new Error("quiescent.role 无效");
    for (const key of counts) if (!Number.isSafeInteger(event[key]) || Number(event[key]) < 0) throw new Error(`quiescent.${key} 无效`);
  } else if (event.type === "childrenDrained") {
    exact(event, ["type", ...counts], "childrenDrained");
    for (const key of counts) if (!Number.isSafeInteger(event[key]) || Number(event[key]) !== 0) throw new Error(`childrenDrained.${key} 必须为零`);
  } else if (event.type === "rustShutdown") {
    exact(event, ["type", "report"], "rustShutdown");
    return { type: "rustShutdown", report: parseWasmSessionShutdown(event.report) };
  } else if (event.type === "finalRelease") {
    if (event.kind === "released") {
      exact(event, ["type", "kind", "finalBytes"], "finalRelease");
      if (!Number.isSafeInteger(event.finalBytes) || Number(event.finalBytes) <= 0) throw new Error("finalRelease.finalBytes 无效");
    } else { exact(event, ["type", "kind"], "finalRelease"); if (event.kind !== "not-allocated") throw new Error("finalRelease.kind 无效"); }
  } else if (event.type === "releaseUnknown") {
    exact(event, ["type", "message"], "releaseUnknown");
    if (typeof event.message !== "string" || event.message.length === 0) throw new Error("releaseUnknown.message 无效");
  } else throw new Error(`未知 controlWorker lifecycle event：${event.type}`);
  return event as unknown as WasmLifecycleObservation;
}

/** Host 持有的轻量句柄不保留已关闭的实际 MessagePort 或 listener。 */
function businessPortHandle(port: MessagePort | null): EngineBusinessPort {
  let current: MessagePort | null = port;
  port = null;
  const listeners = new Set<(event: MessageEvent) => void>();
  function requirePort(): MessagePort {
    if (current === null) throw new Error("controlWorker business port 已关闭");
    return current;
  }
  return {
    addEventListener(type, listener) { requirePort().addEventListener(type, listener); listeners.add(listener); },
    removeEventListener(type, listener) { current?.removeEventListener(type, listener); listeners.delete(listener); },
    postMessage(message) { requirePort().postMessage(message); },
    start() { requirePort().start(); },
    close() {
      const closing = current;
      current = null;
      if (closing === null) return;
      const errors: unknown[] = [];
      for (const listener of listeners) { try { closing.removeEventListener("message", listener); } catch (error) { errors.push(error); } }
      listeners.clear();
      try { closing.close(); } catch (error) { errors.push(error); }
      if (errors.length > 0) throw new AggregateError(errors, "business port 清理失败");
    },
  };
}

/** 主线程只保留业务端口，并通过小控制信封请求 owner 变更生命周期。 */
export function createEngineControlClient(options: {
  readonly runtimeOverride: boolean;
  readonly onLifecycleObservation?: (event: WasmLifecycleObservation) => void;
  readonly onFailure: (error: Error) => void;
  readonly onSpawnFailure: (failure: unknown) => void;
}): {
  readonly instanceId: number;
  readonly enginePort: EngineBusinessPort;
  readonly configured: Promise<void>;
  readonly release: Promise<WasmFinalRelease>;
  start(): void;
  sendInitialization(message: Readonly<Record<string, unknown>>): void;
  bindIngress(generation: number, token: number): Promise<EngineBusinessPort>;
  beginShutdown(until: number): void;
  completeShutdown(): void;
  abortShutdown(error: Error): void;
  terminateAfterRelease(): void;
} {
  const instanceId = positiveSafeInteger(nextInstanceId++, "controlWorker instanceId");
  let engineChannel: MessageChannel | null = new MessageChannel();
  let controlWorker: Worker | null;
  try { controlWorker = new Worker(new URL("./engine-control-worker.ts", import.meta.url), { type: "module", name: `stock-game:control:${instanceId}` }); }
  catch (error) { engineChannel.port1.close(); engineChannel.port2.close(); throw error; }
  const enginePort = businessPortHandle(engineChannel.port1);
  let requestId = 0;
  let failed = false;
  let terminal = false;
  let terminalReceipt: WasmFinalRelease | { readonly kind: "unknown"; readonly message: string } | undefined;
  let businessShutdownCompleted = false;
  let wasConfigured = false;
  let started = false;
  let ingressPort: EngineBusinessPort | null = null;
  let failureCleanupTimer: ReturnType<typeof setTimeout> | undefined;
  let finalObservation: WasmLifecycleObservation | undefined;
  let configuredResolve!: () => void;
  let configuredReject!: (error: Error) => void;
  const configured = new Promise<void>((resolve, reject) => { configuredResolve = resolve; configuredReject = reject; });
  let releaseResolve!: (release: WasmFinalRelease) => void;
  let releaseReject!: (error: Error) => void;
  const release = new Promise<WasmFinalRelease>((resolve, reject) => { releaseResolve = resolve; releaseReject = reject; });
  // 拒绝仍由公开 Promise 与 onFailure 交付；此观察者防止调用方尚未 await 时产生 unhandled rejection。
  void configured.then(undefined, () => undefined);
  void release.then(undefined, () => undefined);
  const ingressWaiters = new Map<number, { resolve: (port: EngineBusinessPort) => void; reject: (error: Error) => void; timer: ReturnType<typeof setTimeout> }>();
  const onWorkerError = (event: Event) => fail(new Error(`controlWorker 执行失败：${(event as ErrorEvent).message}`, { cause: event }));
  const onWorkerMessageError = (event: Event) => fail(new Error("controlWorker 收到无法解码的消息", { cause: event }));

  function closeBusinessPorts(error: Error): void {
    const errors: unknown[] = [];
    try { enginePort.close(); } catch (cleanupError) { errors.push(cleanupError); }
    const closingIngress = ingressPort; ingressPort = null;
    try { closingIngress?.close(); } catch (cleanupError) { errors.push(cleanupError); }
    for (const waiter of ingressWaiters.values()) { clearTimeout(waiter.timer); waiter.reject(error); }
    ingressWaiters.clear();
    if (errors.length > 0) throw new AggregateError(errors, "controlWorker 业务端清理失败");
  }
  function cleanup(): void {
    if (failureCleanupTimer !== undefined) clearTimeout(failureCleanupTimer);
    failureCleanupTimer = undefined;
    const errors: unknown[] = [];
    try { closeBusinessPorts(new Error("controlWorker 已关闭")); } catch (error) { errors.push(error); }
    const closing = controlWorker;
    controlWorker = null;
    if (closing !== null) {
      for (const action of [() => closing.removeEventListener("message", receive), () => closing.removeEventListener("error", onWorkerError), () => closing.removeEventListener("messageerror", onWorkerMessageError)]) {
        try { action(); } catch (error) { errors.push(error); }
      }
      try { closing.terminate(); } catch (error) { errors.push(error); }
    }
    if (errors.length > 0) throw new AggregateError(errors, "controlWorker 主线程资源清理失败");
  }

  function fail(error: Error): void {
    if (failed || terminal) return;
    failed = true;
    configuredReject(error);
    try { closeBusinessPorts(error); }
    catch (cleanupError) { error = new AggregateError([error, cleanupError], "controlWorker 故障且业务端清理失败"); }
    releaseReject(error);
    if (terminalReceipt !== undefined) { terminal = true; cleanup(); options.onFailure(error); return; }
    // 先让 owner 清理 nested Worker；硬故障无收据时只保留 unknown，并在同一 2s 内撤下本地引用。
    failureCleanupTimer = setTimeout(cleanup, 2000);
    try { controlWorker?.postMessage({ type: "abort", instanceId }); }
    catch (abortError) { cleanup(); options.onFailure(new AggregateError([error, abortError], "controlWorker 故障且清理命令无法投递")); return; }
    options.onFailure(error);
  }
  function finishTerminal(): void {
    const result = terminalReceipt;
    if (result === undefined || terminal || !(businessShutdownCompleted || failed || result.kind === "unknown")) return;
    try { cleanup(); }
    catch (error) {
      const detail = error instanceof Error ? error : new Error("controlWorker 主线程清理失败", { cause: error });
      terminal = true; releaseReject(detail); options.onFailure(detail); return;
    }
    terminal = true;
    if (!wasConfigured) configuredReject(new Error("controlWorker 未在 terminal 前完成 configured"));
    if (result.kind === "unknown") releaseReject(new Error(`controlWorker release unknown: ${result.message}`));
    else if (!failed) releaseResolve(result);
    if (finalObservation !== undefined) {
      try { options.onLifecycleObservation?.(finalObservation); }
      catch (error) { options.onFailure(error instanceof Error ? error : new Error("release lifecycle observer 失败", { cause: error })); }
    }
  }
  function receive(event: MessageEvent<unknown>): void {
    if (terminal) return;
    try {
      if (event.data === null || typeof event.data !== "object" || Array.isArray(event.data)) throw new Error("controlWorker lifecycle 消息必须是对象");
      const value = event.data as OwnerMessage;
      if (value.instanceId !== instanceId || typeof value.type !== "string") throw new Error("controlWorker lifecycle 身份或 type 无效");
      if (value.type === "configured") {
        if (Object.keys(value).sort().join(",") !== "instanceId,type") throw new Error("controlWorker configured 字段无效");
        if (wasConfigured) throw new Error("controlWorker configured 重复");
        wasConfigured = true;
        configuredResolve();
        return;
      }
      if (value.type === "lifecycle") {
        if (Object.keys(value).sort().join(",") !== "event,instanceId,type") throw new Error("controlWorker lifecycle 字段无效");
        const observation = lifecycleEvent(value.event);
        if (observation.type === "finalRelease" || observation.type === "releaseUnknown") finalObservation = observation;
        else options.onLifecycleObservation?.(observation);
        return;
      }
      if (value.type === "ingressBound") {
        const hasPort = Object.hasOwn(value, "port");
        if (Object.keys(value).sort().join(",") !== (hasPort ? "instanceId,port,requestId,type" : "instanceId,requestId,type")) throw new Error("controlWorker ingressBound 字段无效");
        const id = positiveSafeInteger(value.requestId, "controlWorker requestId");
        const waiter = ingressWaiters.get(id);
        if (waiter === undefined || hasPort && !(value.port instanceof MessagePort)) throw new Error("controlWorker ingressBound request/port 无效");
        ingressWaiters.delete(id);
        clearTimeout(waiter.timer);
        if (hasPort) {
          if (ingressPort !== null) throw new Error("controlWorker 重复交付 ingress 业务端口");
          ingressPort = businessPortHandle(value.port as MessagePort); ingressPort.start();
        }
        if (ingressPort === null) throw new Error("controlWorker ingressBound 缺少首次业务端口");
        waiter.resolve(ingressPort);
        return;
      }
      if (value.type === "terminal") {
        if (Object.keys(value).sort().join(",") !== "instanceId,release,type") throw new Error("controlWorker terminal 字段无效");
        if (terminalReceipt !== undefined) throw new Error("controlWorker terminal 重复");
        const raw = value.release;
        if (raw === null || typeof raw !== "object" || Array.isArray(raw)) throw new Error("controlWorker terminal release 无效");
        const result = raw as Record<string, unknown>;
        if (result.kind === "released") {
          exact(result, ["kind", "finalBytes", "growthStopped", "referencesDropped"], "terminal.released");
          if (!Number.isSafeInteger(result.finalBytes) || Number(result.finalBytes) <= 0 || result.growthStopped !== true || result.referencesDropped !== true) throw new Error("controlWorker released 收据字段无效");
        } else if (result.kind === "not-allocated") exact(result, ["kind"], "terminal.not-allocated");
        else if (result.kind === "unknown") {
          exact(result, ["kind", "message"], "terminal.unknown");
          if (typeof result.message !== "string" || result.message.length === 0) throw new Error("controlWorker unknown 收据字段无效");
        } else throw new Error("controlWorker terminal release.kind 无效");
        terminalReceipt = result as unknown as NonNullable<typeof terminalReceipt>;
        finishTerminal();
        return;
      }
      if (value.type === "failure") {
        if (Object.keys(value).sort().join(",") !== "failure,instanceId,type") throw new Error("controlWorker failure 字段无效");
        const detail = parseHostFailure(value.failure, "engine-control-client.failure");
        throw new Error(`${detail.code} @ ${detail.where}: ${detail.message}`, { cause: detail });
      }
      if (value.type === "spawnFailure") {
        if (Object.keys(value).sort().join(",") !== "failure,instanceId,type") throw new Error("controlWorker spawnFailure 字段无效");
        options.onSpawnFailure(parseHostFailure(value.failure, "engine-control-client.spawnFailure"));
        return;
      }
      throw new Error(`未知 controlWorker lifecycle 消息：${value.type}`);
    } catch (error) { fail(error instanceof Error ? error : new Error("controlWorker 协议失败", { cause: error })); }
  }
  try {
    enginePort.start();
    controlWorker.addEventListener("message", receive);
    controlWorker.addEventListener("error", onWorkerError);
    controlWorker.addEventListener("messageerror", onWorkerMessageError);
  }
  catch (error) {
    const errors: unknown[] = [error];
    try { engineChannel.port2.close(); } catch (cleanupError) { errors.push(cleanupError); }
    try { cleanup(); } catch (cleanupError) { errors.push(cleanupError); }
    engineChannel = null;
    if (errors.length > 1) throw new AggregateError(errors, "controlWorker start 前构造失败且清理失败");
    throw error instanceof Error ? error : new Error("controlWorker 构造失败", { cause: error });
  }

  return {
    instanceId, enginePort, configured, release,
    start() {
      if (started || controlWorker === null || engineChannel === null) throw new Error("controlWorker start 只允许首次调用");
      started = true;
      try {
        controlWorker.postMessage({ type: "start", instanceId, runtimeOverride: options.runtimeOverride, dataPort: engineChannel.port2 }, [engineChannel.port2]);
        engineChannel = null;
      } catch (error) {
        const errors: unknown[] = [error];
        try { engineChannel?.port2.close(); } catch (cleanupError) { errors.push(cleanupError); }
        engineChannel = null;
        try { cleanup(); } catch (cleanupError) { errors.push(cleanupError); }
        const detail = error instanceof Error ? error : new Error("发送 controlWorker start 失败", { cause: error });
        terminal = true;
        configuredReject(detail);
        // postMessage 抛错表示 start 未投递，owner 没有开始分配 Memory。
        if (errors.length === 1) releaseResolve({ kind: "not-allocated" });
        else releaseReject(new AggregateError(errors, "controlWorker start 失败且清理失败"));
        throw detail;
      }
    },
    sendInitialization(message) { enginePort.postMessage(message); },
    bindIngress(generation, token) {
      if (failed || terminal || controlWorker === null) return Promise.reject(new Error("controlWorker 已关闭，不能 bindIngress"));
      const id = positiveSafeInteger(++requestId, "controlWorker requestId");
      return new Promise<EngineBusinessPort>((resolve, reject) => {
        const waiter = { resolve, reject, timer: setTimeout(() => {
          ingressWaiters.delete(id);
          const error = new Error("controlWorker bindIngress 超过 10000ms");
          reject(error); fail(error);
        }, 10_000) };
        ingressWaiters.set(id, waiter);
        try { controlWorker!.postMessage({ type: "bindIngress", instanceId, requestId: id, generation, token }); }
        catch (error) { ingressWaiters.delete(id); clearTimeout(waiter.timer); reject(error instanceof Error ? error : new Error("发送 bindIngress 失败", { cause: error })); }
      });
    },
    beginShutdown(until) {
      if (!Number.isFinite(until) || until < 0) throw new Error("controlWorker shutdown deadline 无效");
      if (terminal || failed || terminalReceipt !== undefined) return;
      if (controlWorker === null) throw new Error("controlWorker 引用已撤下");
      const deadline = performance.timeOrigin + until;
      controlWorker.postMessage({ type: "beginShutdown", instanceId, deadline });
    },
    completeShutdown() { businessShutdownCompleted = true; finishTerminal(); },
    abortShutdown(error) { fail(error); },
    terminateAfterRelease() {
      if (!terminal) throw new Error("controlWorker 真实 terminal 收据尚未到达");
      cleanup();
    },
  };
}
