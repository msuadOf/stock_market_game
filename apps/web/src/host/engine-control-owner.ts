import { sharedControlWord } from "../../../../packages/libos/browser/shared-substrate-publisher.ts";
import { createEngineSupervisor } from "./engine-supervisor.ts";
import type { WasmLifecycleObservation } from "./wasm-lifecycle-observation.ts";
import type { WasmFinalRelease } from "./wasm-document-memory-owner.ts";

export interface EngineControlOwnerDependencies {
  readonly createEngineWorker: (name: string) => Worker;
  readonly createChildWorker: (name: string) => Worker;
  readonly createPublisherWorker?: (name: string) => Worker;
  readonly createIngressWorker: (name: string) => Worker;
  readonly createChannel: () => MessageChannel;
  readonly postMessage: (value: Readonly<Record<string, unknown>>, transfer?: Transferable[]) => void;
  readonly addMessageListener: (listener: (event: MessageEvent<unknown>) => void) => void;
  readonly reportError: (error: unknown) => void;
}

type ControlRequest = { readonly type: string; readonly [key: string]: unknown };
type ActiveOwner = {
  readonly instanceId: number;
  supervisor: ReturnType<typeof createEngineSupervisor> | null;
  engine: Worker | null;
  engineCreated: boolean;
  ingress: Worker | null;
  publisher: Worker | null;
  publisherGeneration: number;
  publisherEpoch: number;
  publisherToken: number;
  publisherControlAddress: number;
  pendingPublisherBind: ControlRequest | null;
  readonly publisherBindings: Map<number, number>;
  publisherQuiescent: boolean;
  ingressPort: MessagePort | null;
  ingressPortSent: boolean;
  ingressGeneration: number;
  readonly ingressConfigurations: Map<number, number>;
  readonly listeners: Array<readonly [EventTarget, string, EventListener]>;
  readonly ports: Set<MessagePort>;
  engineQuiescent: boolean;
  ingressQuiescent: boolean;
  childrenDrained: boolean;
  shutdownDeadline: number;
  finalizing: boolean;
  terminal: boolean;
  failing: boolean;
  failurePending: boolean;
  configured: boolean;
  shutdownTimer?: ReturnType<typeof setTimeout>;
};

export function createEngineControlOwner(dependencies: EngineControlOwnerDependencies): void {
let owner: ActiveOwner | undefined;
let finalReleaseEvent: WasmLifecycleObservation | undefined;

function exact(value: Record<string, unknown>, keys: readonly string[], type: string): void {
  if (Object.keys(value).sort().join(",") !== [...keys].sort().join(",")) throw new Error(`${type} 字段无效`);
}
function safe(value: unknown, label: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 1) throw new Error(`${label} 必须为正安全整数`);
  return value;
}
function post(value: Readonly<Record<string, unknown>>, transfer: Transferable[] = []): void {
  dependencies.postMessage(value, transfer);
}
function listen(active: ActiveOwner, target: EventTarget, type: string, listener: EventListener): void {
  target.addEventListener(type, listener);
  active.listeners.push([target, type, listener]);
}
function dropReferences(active: ActiveOwner): void {
  if (active.shutdownTimer !== undefined) clearTimeout(active.shutdownTimer);
  active.shutdownTimer = undefined;
  const errors: unknown[] = [];
  for (const [target, type, listener] of active.listeners) {
    try { target.removeEventListener(type, listener); } catch (error) { errors.push(error); }
  }
  active.listeners.length = 0;
  for (const port of active.ports) { try { port.close(); } catch (error) { errors.push(error); } }
  active.ports.clear();
  active.ingressConfigurations.clear();
  active.publisherBindings.clear();
  active.pendingPublisherBind = null;
  active.publisher = null;
  active.ingressPort = null;
  active.supervisor = null;
  active.engine = null;
  active.ingress = null;
  if (errors.length > 0) throw new AggregateError(errors, "controlWorker 引用清理失败");
}
function serializeFailure(error: unknown, fallbackWhere: string): Readonly<Record<string, unknown>> {
  if (error !== null && typeof error === "object") {
    const source = error as Record<string, unknown>;
    if (typeof source.code === "string" && typeof source.where === "string" && typeof source.message === "string") {
      return structuredClone(source);
    }
  }
  const detail = error instanceof Error ? error : new Error("controlWorker 发生非 Error 故障", { cause: error });
  return { code: "WASM_CONTROL_WORKER_FAILURE", where: fallbackWhere, message: detail.message, cause: detail };
}
function workerEventFailure(event: Event, role: "engine" | "ingress" | "publisher", kind: "error" | "messageerror"): Readonly<Record<string, unknown>> {
  const error = event as ErrorEvent;
  const message = typeof error.message === "string" && error.message.length > 0 ? error.message : `${role} Worker ${kind} 事件`;
  return { code: "WASM_WORKER_EVENT", where: `engine-control-worker.${role}.${kind}`, message,
    cause: { type: event.type, ...(typeof error.filename === "string" ? { filename: error.filename } : {}),
      ...(typeof error.lineno === "number" ? { lineno: error.lineno } : {}), ...(typeof error.colno === "number" ? { colno: error.colno } : {}) } };
}
function sendSpawnFailure(active: ActiveOwner, failure: unknown): void {
  try { post({ type: "spawnFailure", instanceId: active.instanceId, failure: serializeFailure(failure, "engine-control-worker.spawn") }); }
  catch (error) { sendFailure(active, error); }
}
function sendFailure(active: ActiveOwner, error: unknown, supervisorDisposed = false): void {
  if (active.terminal || active.failing) return;
  const failure = serializeFailure(error, "engine-control-worker");
  const detail = error instanceof Error ? error : new Error(String(failure.message), { cause: error });
  if (!supervisorDisposed && active.supervisor !== null) {
    try {
      const supervisorFailure = { code: "WASM_CONTROL_WORKER_FAILURE", where: "engine-control-worker", message: detail.message, cause: detail };
      if (active.shutdownDeadline !== Infinity && performance.now() >= active.shutdownDeadline) active.supervisor.finishFailureAtDeadline(supervisorFailure);
      else {
        if (active.shutdownDeadline === Infinity) {
          active.shutdownDeadline = performance.now() + 2000;
          active.supervisor.setDeadline(active.shutdownDeadline);
        }
        active.supervisor.discard(supervisorFailure);
      }
      if (active.terminal || active.failurePending) return;
    } catch (releaseError) { dependencies.reportError(new AggregateError([detail, releaseError], "controlWorker 故障后释放失败")); }
  }
  active.failing = true;
  try { post({ type: "failure", instanceId: active.instanceId, failure }); }
  catch (reportError) { dependencies.reportError(new AggregateError([detail, reportError], "controlWorker 错误无法投递")); }
  // supervisor 尚未接管的构造中资源同样必须尝试终止；收据始终保留 unknown。
  if (active.supervisor === null) {
    try { active.engine?.terminate(); active.ingress?.terminate(); active.publisher?.terminate(); }
    catch (releaseError) { dependencies.reportError(new AggregateError([detail, releaseError], "未登记 Worker 清理失败")); }
  }
  try { dropReferences(active); }
  catch (releaseError) { dependencies.reportError(new AggregateError([detail, releaseError], "controlWorker 清理失败")); }
  if (!active.engineCreated) {
    post({ type: "lifecycle", instanceId: active.instanceId, event: { type: "finalRelease", kind: "not-allocated" } });
    terminal(active, { kind: "not-allocated" });
  } else {
    if (finalReleaseEvent !== undefined) post({ type: "lifecycle", instanceId: active.instanceId, event: finalReleaseEvent });
    terminal(active, { kind: "unknown", message: finalReleaseEvent?.type === "releaseUnknown" ? finalReleaseEvent.message : detail.message });
  }
}
function lifecycle(active: ActiveOwner, event: WasmLifecycleObservation): void {
  if (event.type === "finalRelease" || event.type === "releaseUnknown") { finalReleaseEvent = event; return; }
  if (event.type === "allocationConfigured" && event.role === "engine" && !active.configured) {
    active.configured = true;
    post({ type: "configured", instanceId: active.instanceId });
  }
  post({ type: "lifecycle", instanceId: active.instanceId, event });
}
function finalize(active: ActiveOwner): void {
  if (active.terminal || active.finalizing || active.failurePending || !active.engineQuiescent || !active.ingressQuiescent || !active.publisherQuiescent || !active.childrenDrained) return;
  active.finalizing = true;
  try {
    const supervisor = active.supervisor;
    if (supervisor === null) throw new Error("controlWorker finalize 时 supervisor 缺失");
    const receipt = supervisor.dispose();
    dropReferences(active);
    if (finalReleaseEvent !== undefined) post({ type: "lifecycle", instanceId: active.instanceId, event: finalReleaseEvent });
    terminal(active, receipt instanceof Error ? { kind: "unknown", message: receipt.message } : receipt);
  } catch (error) { sendFailure(active, error); }
}
function terminal(active: ActiveOwner, release: WasmFinalRelease | { readonly kind: "unknown"; readonly message: string }): void {
  if (active.terminal) return;
  active.terminal = true;
  post({ type: "terminal", instanceId: active.instanceId, release });
}
function initialize(value: unknown): ActiveOwner {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("controlWorker start 必须是对象");
  const request = value as ControlRequest;
  exact(request, ["type", "instanceId", "runtimeOverride", "dataPort"], "controlWorker start");
  const instanceId = safe(request.instanceId, "controlWorker instanceId");
  if (request.type !== "start" || typeof request.runtimeOverride !== "boolean" || !(request.dataPort instanceof MessagePort)) throw new Error("controlWorker start 内容无效");
  const active: ActiveOwner = { instanceId, supervisor: null, engine: null, engineCreated: false, ingress: null, ingressPort: null,
    ingressPortSent: false, ingressGeneration: 0, ingressConfigurations: new Map(), publisher: null, publisherGeneration: 0, publisherEpoch: 0, publisherToken: 0, publisherControlAddress: 0, pendingPublisherBind: null, publisherBindings: new Map(), publisherQuiescent: false, listeners: [], ports: new Set([request.dataPort as MessagePort]), engineQuiescent: false, ingressQuiescent: false, childrenDrained: false, shutdownDeadline: Infinity, finalizing: false, terminal: false, failing: false, failurePending: false, configured: false };
  owner = active;
  const engine = dependencies.createEngineWorker(`stock-game:${instanceId}:engine`);
  active.engineCreated = true;
  active.engine = engine;
  let supervisor: ReturnType<typeof createEngineSupervisor>;
  try { supervisor = createEngineSupervisor({
    engine, instanceId,
    createChildWorker: (name) => {
      if (name === undefined) throw new Error("controlWorker child 缺少精确实例身份 name");
      return dependencies.createChildWorker(name);
    },
    createChannel: dependencies.createChannel,
    onFailurePending(failure) { active.failurePending = true; post({ type: "failurePending", instanceId: active.instanceId, failure: serializeFailure(failure, "engine-control-worker.pending") }); },
    onFatalFailure(failure) { sendFailure(active, failure, true); },
    onSpawnFailure(failure) { sendSpawnFailure(active, failure); },
    onLifecycleObservation(event) { lifecycle(active, event); },
  }); } catch (error) {
    // createEngineSupervisor 的 attach 已终止构造中 engine；禁止重复 terminate。
    active.engine = null;
    throw error;
  }
  active.supervisor = supervisor;
  listen(active, engine, "message", (event) => {
    const message = (event as MessageEvent<unknown>).data;
    if (message === null || typeof message !== "object" || Array.isArray(message)) { sendFailure(active, new Error("engine 全局消息必须是控制对象")); return; }
    const source = message as Record<string, unknown>;
    if (source.type !== "engineLifecycle") {
      if (!supervisor.handleGlobalMessage(engine, message)) sendFailure(active, new Error("engine 全局消息不符合生命周期或 supervisor 控制协议"));
      return;
    }
    if (source.instanceId !== instanceId) { sendFailure(active, new Error("engine lifecycle instanceId 不匹配")); return; }
    try {
      exact(source, ["type", "instanceId", "state"], "engineLifecycle");
      if (source.state === "quiescent") {
        if (active.engineQuiescent) throw new Error("engine quiescent 重复");
        active.engineQuiescent = true;
        supervisor.markQuiescent(engine);
        finalize(active);
      } else if (source.state === "unallocated") {
        if (active.engineQuiescent) throw new Error("engine unallocated 重复");
        active.engineQuiescent = true;
        supervisor.markEngineUnallocated(engine);
        finalize(active);
      } else throw new Error("engineLifecycle.state 无效");
    } catch (error) { sendFailure(active, error); }
  });
  listen(active, engine, "error", (event) => sendFailure(active, workerEventFailure(event, "engine", "error")));
  listen(active, engine, "messageerror", (event) => sendFailure(active, workerEventFailure(event, "engine", "messageerror")));
  supervisor.bootstrapEngineTransport(request.dataPort as MessagePort, instanceId, request.runtimeOverride);
  active.ports.delete(request.dataPort as MessagePort);
  return active;
}
function bindIngress(active: ActiveOwner, request: ControlRequest): void {
  exact(request, ["type", "instanceId", "requestId", "generation", "token"], "bindIngress");
  const requestId = safe(request.requestId, "bindIngress requestId");
  const generation = safe(request.generation, "bindIngress generation");
  if (request.instanceId !== active.instanceId || request.type !== "bindIngress" || typeof request.token !== "number" || !Number.isInteger(request.token) || request.token < 1 || request.token > 0xffffffff) throw new Error("bindIngress 内容无效");
  if (active.shutdownDeadline !== Infinity || generation <= active.ingressGeneration) throw new Error("bindIngress 阶段或 generation 无效");
  active.ingressGeneration = generation;
  active.ingressConfigurations.set(generation, request.token);
  if (active.ingress === null) {
    const ingress = dependencies.createIngressWorker(`stock-game:${active.instanceId}:ingress`);
    active.ingress = ingress;
    // 先接管 Worker，再创建 Channel；后续构造失败由 supervisor 清理全部本人资源。
    active.supervisor!.attachIngress(ingress);
    const channel = dependencies.createChannel();
    active.ports.add(channel.port1); active.ports.add(channel.port2);
    active.ingressPort = channel.port1;
    const substrate = active.supervisor!.getSubstrate();
    listen(active, ingress, "message", (event) => {
      const value = (event as MessageEvent<unknown>).data;
      if (value === null || typeof value !== "object" || Array.isArray(value)) { sendFailure(active, new Error("ingress 全局消息必须是控制对象")); return; }
      const message = value as Record<string, unknown>;
      try {
        if (message.type !== "ingressLifecycle" || message.instanceId !== active.instanceId) throw new Error("ingress lifecycle 身份或 type 无效");
        exact(message, message.state === "configured" ? ["type", "instanceId", "state", "generation", "token"] : ["type", "instanceId", "state"], "ingressLifecycle");
        if (message.state !== "configured" && message.state !== "quiescent") throw new Error("ingressLifecycle.state 无效");
        if (message.state === "configured") {
          if (typeof message.generation !== "number" || !active.ingressConfigurations.has(message.generation) || active.ingressConfigurations.get(message.generation) !== message.token) throw new Error("ingress configured 无在途 bind 或 generation/token 不匹配");
          active.ingressConfigurations.delete(message.generation);
          active.supervisor!.observeIngressConfigured(ingress);
        }
        else {
          if (active.ingressQuiescent) throw new Error("ingress quiescent 重复");
          active.ingressQuiescent = true;
          active.supervisor!.markQuiescent(ingress);
          finalize(active);
        }
      } catch (error) { sendFailure(active, error); }
    });
    listen(active, ingress, "error", (event) => sendFailure(active, workerEventFailure(event, "ingress", "error")));
    listen(active, ingress, "messageerror", (event) => sendFailure(active, workerEventFailure(event, "ingress", "messageerror")));
    if (substrate.primary === undefined) throw new Error("ingress 缺少预登记 tombstone");
    ingress.postMessage({ type: "bindIngressTransport", instanceId: active.instanceId, dataPort: channel.port2,
      module: substrate.module, memory: substrate.memory, layout: substrate.layout, diagnostics: substrate.diagnostics, primary: substrate.primary.ingress }, [channel.port2]);
    active.ports.delete(channel.port2);
  }
  if (!active.ingressPortSent) {
    const port = active.ingressPort;
    if (port === null) throw new Error("ingress 业务端口缺失");
    post({ type: "ingressBound", instanceId: active.instanceId, requestId, port }, [port]);
    active.ingressPortSent = true;
    active.ports.delete(port);
    active.ingressPort = null;
  } else post({ type: "ingressBound", instanceId: active.instanceId, requestId });
}

function postPublisherObserverFailure(active: ActiveOwner, generation: number, message: string): void {
  if (active.terminal || active.failing || generation < active.publisherGeneration) return;
  if (generation !== active.publisherGeneration || !message.trim()) throw new Error("发布观察者故障身份或诊断无效");
  post({ type: "publisherObserverFailure", instanceId: active.instanceId, publisherEpoch: active.publisherEpoch, generation, message });
}

function bindPublisher(active: ActiveOwner, request: ControlRequest): void {
  exact(request, ["type", "instanceId", "requestId", "generation", "token", "controlAddress"], "bindPublisher");
  const requestId = safe(request.requestId, "bindPublisher requestId");
  const generation = safe(request.generation, "bindPublisher generation");
  const token = safe(request.token, "bindPublisher token");
  if (token > 0xffffffff || active.shutdownDeadline !== Infinity || active.failurePending || generation < active.publisherGeneration)
    throw new Error("bindPublisher 阶段、token 或 generation 无效");
  if (active.pendingPublisherBind !== null) throw new Error("publisher退出证明仍在途，禁止叠加bind");
  const controlAddress = safe(request.controlAddress, "bindPublisher controlAddress");
  if (generation === active.publisherGeneration) {
    if (active.publisher === null || token !== active.publisherToken || controlAddress !== active.publisherControlAddress)
      throw new Error("同代publisher重建必须保留真实token/control地址");
    if (!active.publisherQuiescent) {
      // 业务Channel的关闭ACK可能先于global退出证明；保留请求而不伪造已退出。
      active.pendingPublisherBind = request;
      return;
    }
  }
  const supervisor = active.supervisor;
  if (supervisor === null) throw new Error("controlWorker 缺少已配置的 supervisor");
  const substrate = supervisor.getSubstrate();
  const controlSignal = sharedControlWord(substrate.memory, controlAddress);
  if (active.publisher !== null && active.publisherQuiescent) {
    const retired = active.publisher;
    supervisor.retireQuiescentPublisher(retired);
    for (let index=active.listeners.length-1; index>=0; index--) {
      const [target,type,listener]=active.listeners[index]!;
      if (target === retired) { target.removeEventListener(type,listener); active.listeners.splice(index,1); }
    }
    active.publisher = null;
    active.publisherBindings.clear();
  }
  active.publisherGeneration = generation;
  active.publisherToken = token;
  active.publisherControlAddress = controlAddress;
  active.publisherBindings.set(generation, token);
  if (active.publisher !== null) {
    post({ type: "publisherBound", instanceId: active.instanceId, requestId, publisherEpoch: active.publisherEpoch, controlSignal });
    return;
  }
  if (dependencies.createPublisherWorker === undefined) throw new Error("controlWorker 缺少独立发布 Worker 工厂");
  if (substrate.primary === undefined) throw new Error("发布 Worker 缺少预登记的独立 tombstone");
  const previousPublisherEpoch = active.publisherEpoch;
  const publisherEpoch = safe(previousPublisherEpoch + 1, "publisher物理epoch");
  const publisher = dependencies.createPublisherWorker(`stock-game:${active.instanceId}:publisher:${publisherEpoch}`);
  active.publisher = publisher;
  active.publisherEpoch = publisherEpoch;
  active.publisherQuiescent = false;
  supervisor.attachPublisher(publisher);
  const channel = dependencies.createChannel();
  active.ports.add(channel.port1); active.ports.add(channel.port2);
  const current = () => active.publisher === publisher && active.publisherEpoch === publisherEpoch;
  listen(active, publisher, "message", (event) => {
    if (!current()) return;
    try {
      const raw: unknown = (event as MessageEvent<unknown>).data;
      if (raw === null || typeof raw !== "object" || Array.isArray(raw)) throw new Error("发布 Worker 全局消息必须是控制对象");
      const message = raw as Record<string, unknown>;
      if (message.type !== "publisherLifecycle" || message.instanceId !== active.instanceId) throw new Error("发布 Worker lifecycle 身份或类型无效");
      if (message.state === "configured") {
        exact(message, ["type", "state", "instanceId", "generation", "token"], "publisherLifecycle configured");
        const configured = safe(message.generation, "发布 Worker configured generation");
        const configuredToken = safe(message.token, "发布 Worker configured token");
        if (active.publisherBindings.get(configured) !== configuredToken) throw new Error("发布 Worker configured 没有匹配的在途 generation/token");
        active.publisherBindings.delete(configured);
        supervisor.observePublisherConfigured(publisher);
      } else if (message.state === "observerFailure") {
        exact(message, ["type", "state", "instanceId", "generation", "message"], "publisherLifecycle observerFailure");
        if (typeof message.message !== "string") throw new Error("发布观察者错误缺少诊断");
        const generation = message.generation === 0 ? active.publisherGeneration : safe(message.generation, "发布观察者错误 generation");
        postPublisherObserverFailure(active, generation, message.message);
      } else if (message.state === "quiescent") {
        exact(message, ["type", "state", "instanceId"], "publisherLifecycle quiescent");
        if (active.publisherQuiescent) throw new Error("发布 Worker quiescent 重复");
        active.publisherQuiescent = true;
        supervisor.markQuiescent(publisher);
        const pending = active.pendingPublisherBind;
        active.pendingPublisherBind = null;
        if (pending !== null && active.shutdownDeadline === Infinity) bindPublisher(active, pending);
        finalize(active);
      } else throw new Error("发布 Worker lifecycle 阶段无效");
    } catch (error) { sendFailure(active, error); }
  });
  listen(active, publisher, "error", (event) => { if (current()) sendFailure(active, workerEventFailure(event, "publisher", "error")); });
  listen(active, publisher, "messageerror", () => {
    if (!current()) return;
    // JS 解码错误不能证明共享 Memory 被破坏；保留 owner 并等待真实关闭收据。
    try { postPublisherObserverFailure(active, active.publisherGeneration, "发布 Worker 控制消息无法解码"); }
    catch (error) { sendFailure(active, error); }
  });
  publisher.postMessage({ type: "bindPublisherTransport", instanceId: active.instanceId, dataPort: channel.port2,
    module: substrate.module, memory: substrate.memory, layout: substrate.layout, diagnostics: substrate.diagnostics, primary: substrate.primary.publisher }, [channel.port2]);
  active.ports.delete(channel.port2);
  post({ type: "publisherBound", instanceId: active.instanceId, requestId, publisherEpoch, previousPublisherEpoch, controlSignal, port: channel.port1 }, [channel.port1]);
  active.ports.delete(channel.port1);
}

dependencies.addMessageListener((event: MessageEvent<unknown>) => {
  try {
    if (owner === undefined) { owner = initialize(event.data); return; }
    if (event.data === null || typeof event.data !== "object" || Array.isArray(event.data)) throw new Error("controlWorker 命令必须是对象");
    const request = event.data as ControlRequest;
    if (request.instanceId !== owner.instanceId || typeof request.type !== "string") throw new Error("controlWorker 命令身份或 type 无效");
    if (owner.terminal || owner.failing) return;
    if (request.type === "abort") {
      exact(request, ["type", "instanceId"], "abort");
      sendFailure(owner, new Error("主线程报告控制通道故障，资源释放保持 unknown")); return;
    }
    if (request.type === "shutdownFailure") {
      exact(request, ["type", "instanceId", "message"], "shutdownFailure");
      if (!owner.failurePending || typeof request.message !== "string" || request.message.length === 0) throw new Error("shutdownFailure 内容或阶段无效");
      owner.supervisor!.noteFailureDuringPending({ code: "WASM_SHUTDOWN_FAILURE", where: "engine-control-worker.shutdown", message: request.message });
      return;
    }
    if (request.type === "bindIngress") { bindIngress(owner, request); return; }
    if (request.type === "bindPublisher") { bindPublisher(owner, request); return; }
    if (request.type === "beginShutdown") {
      const active = owner;
      exact(request, ["type", "instanceId", "deadline"], "beginShutdown");
      if (typeof request.deadline !== "number" || !Number.isFinite(request.deadline)) throw new Error("beginShutdown deadlineUnixMs 无效");
      const remaining = Math.max(0, request.deadline - performance.timeOrigin - performance.now());
      owner.shutdownDeadline = Math.min(owner.shutdownDeadline, performance.now() + remaining);
      if (owner.ingress === null) owner.ingressQuiescent = true;
      if (owner.publisher === null) owner.publisherQuiescent = true;
      owner.supervisor!.beginShutdown(owner.shutdownDeadline);
      if (owner.shutdownTimer !== undefined) clearTimeout(owner.shutdownTimer);
      owner.shutdownTimer = setTimeout(() => {
        if (!active.terminal && (!active.engineQuiescent || !active.ingressQuiescent || !active.publisherQuiescent || !active.childrenDrained)) {
          const error = new Error("共享关停期限届满，缺少真实 engine/ingress/publisher quiescence 或 child drain 收据");
          sendFailure(active, error);
        }
      }, Math.max(0, owner.shutdownDeadline - performance.now()));
      void active.supervisor!.waitForChildrenExit(active.shutdownDeadline).then(() => { active.childrenDrained = true; finalize(active); }, (error) => { if (active.failurePending) active.supervisor?.noteFailureDuringPending({ code: "WASM_SHUTDOWN_FAILURE", where: "engine-control-worker.shutdown", message: String(error) }); else sendFailure(active, error); });
      return;
    }
    throw new Error(`未知 controlWorker 命令：${request.type}`);
  } catch (error) {
    if (owner === undefined) throw error;
    sendFailure(owner, error);
  }
});
}
