import type { FacadeChildBootstrapMessage } from "./facade-child-bootstrap.ts";
import { FACADE_BOOTSTRAP_REPORTER_FAILURE } from "./facade-child-bootstrap.ts";
import type { FacadeSlotLayout, FacadeThreadIdentity } from "./facade-thread-binding.ts";
import type { HostFailure } from "./host-update.ts";
import { parseFacadeSupervisorControl, type FacadeSupervisorControlMessage } from "./facade-supervisor-control.ts";
import { parseHostFailure } from "./protocol-failure.ts";
import { classifyWasmFailure, describeWasmFailure } from "./wasm-failure.ts";
import { answerFail, answerOk, createSupervisorState, evaluateOkRecovery, onWorkerExit, recoverPending, registerChild, writeDead, type FacadeArenaView } from "./facade-supervisor.ts";
import { CAPACITY, GEN_BITS, GEN_BITS_DEFAULT, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE, SLOT_JOIN_INDEX, SlotState, JoinState, decodeGen, decodeJoinState, encodeW, slotWordOffset, slotFieldOffset, joinWordOffset } from "./facade-slot-protocol.ts";

/** 只分配宿主错误载荷，不改变 Rust 的 WorkerFailed 判定。 */
export const FacadeSpawnError = Object.freeze({ WORKER_CONSTRUCT: 1, SCRIPT_LOAD: 2, INSTANTIATE: 3, SUPERVISOR_SHUTDOWN: 4 });
export function allocateFacadeWorkerId(next: number): { workerId: number; next: number } {
  if (!Number.isInteger(next) || next < 1 || next > 0xffffffff) throw new Error("facade Worker id 已耗尽或非法，禁止回绕");
  return { workerId: next, next: next + 1 };
}
export interface FacadeSubstrate {
  readonly module: WebAssembly.Module;
  readonly memory: WebAssembly.Memory;
  readonly layout: FacadeSlotLayout;
  readonly diagnostics: boolean;
}
export interface SupervisorTimers {
  now(): number;
  setTimeout(callback: () => void, milliseconds: number): number;
  clearTimeout(id: number): void;
}
export interface EngineSupervisorOptions {
  readonly engine: Worker;
  readonly createChildWorker: () => Worker;
  readonly createChannel?: () => MessageChannel;
  readonly onFatalFailure: (failure: HostFailure) => void;
  readonly onSpawnFailure: (failure: HostFailure) => void;
  readonly reportError?: (error: Error) => void;
  /** 仅用于确定性短 fixture；产品始终使用默认时钟及期限。 */
  readonly timers?: SupervisorTimers;
}
interface Resource {
  readonly worker: Worker;
  readonly role: "engine" | "child" | "ingress";
  readonly identity?: Readonly<FacadeThreadIdentity>;
  readonly port?: MessagePort;
  localPort?: MessagePort;
  readonly listeners: Array<readonly [EventTarget, string, EventListener]>;
  readyTimer?: number;
  ready: boolean;
  doneObservedAt?: number;
}

/** 主线程持全部真实资源；协议 manifest 只保存 W0 的 slot/gen 事实。 */
export function createEngineSupervisor(options: EngineSupervisorOptions) {
  const { createChildWorker, onFatalFailure, onSpawnFailure, reportError } = options;
  const timers: SupervisorTimers = options.timers === undefined ? {
    now: () => performance.now(),
    setTimeout: (callback, milliseconds) => window.setTimeout(callback, milliseconds),
    clearTimeout: (id) => window.clearTimeout(id),
  } : options.timers;
  const createChannel = options.createChannel === undefined ? () => new MessageChannel() : options.createChannel;
  const resources = new Map<Worker, Resource>();
  const protocol = createSupervisorState();
  let substrate: FacadeSubstrate | undefined;
  let engineResource: Resource | undefined;
  let nextWorkerId = 1;
  let disposed = false, shuttingDown = false;
  let deadline = Infinity;
  let sweepTimer: number | undefined;

  function failure(code: string, where: string, error: unknown, context?: unknown): HostFailure {
    const location = `engine-supervisor.${where}`;
    const classified = classifyWasmFailure(error);
    if (classified.kind === "structured") {
      const snapshot: Record<string, unknown> = { code: classified.code, message: classified.message };
      try {
        const source = error as Readonly<Record<string, unknown>>;
        for (const key of ["where", "cause", "context", "recoverable", "recoveryActions"]) {
          if (Object.hasOwn(source, key)) snapshot[key] = structuredClone(source[key]);
        }
        return parseHostFailure(snapshot, location);
      } catch (detailError) {
        return { code: "FACADE_FAILURE_REPORTER_ERROR", where: location,
          message: `读取或克隆结构化错误详情失败：${describeWasmFailure(detailError)}；原错误 ${classified.code}：${classified.message}`, cause: snapshot, context };
      }
    }
    if (classified.kind === "access-error") return { code: "FACADE_FAILURE_REPORTER_ERROR", where: location,
      message: `读取错误字段 ${classified.field} 失败：${classified.reason}`, cause: classified.cause, context };
    const snapshot: Record<string, unknown> = { ...classified.cause };
    try {
      if (error instanceof Error) {
        snapshot.name = error.name; snapshot.stack = error.stack;
        if (Object.hasOwn(error, "cause")) snapshot.cause = structuredClone(error.cause);
      }
    } catch (detailError) {
      return { code: "FACADE_FAILURE_REPORTER_ERROR", where: location,
        message: `克隆 Error 详情失败：${describeWasmFailure(detailError)}；原错误：${classified.description}`, cause: snapshot, context };
    }
    if (classified.description.trim().length === 0) return { code: "FACADE_FAILURE_REPORTER_ERROR", where: location,
      message: "原错误没有可读的非空 message", cause: { ...snapshot, message: classified.description }, context };
    return { code, where: location, message: classified.description, cause: snapshot,
      context, recoverable: false, recoveryActions: ["请保留错误详情并反馈"] };
  }
  function arena(): FacadeArenaView {
    if (substrate === undefined) throw new Error("facade substrate 尚未装入");
    const { memory, layout } = substrate;
    return { words: new Int32Array(memory.buffer, layout.baseAddr, layout.headerWords + layout.capacity * (layout.slotStride + layout.joinStride)), capacity: layout.capacity, genBits: layout.genBits };
  }
  function cleanup(resource: Resource, terminate: boolean): void {
    if (!resources.delete(resource.worker)) return;
    if (resource.readyTimer !== undefined) { timers.clearTimeout(resource.readyTimer); resource.readyTimer = undefined; }
    for (const [target, type, listener] of resource.listeners) target.removeEventListener(type, listener);
    resource.listeners.length = 0;
    resource.port?.close();
    resource.localPort?.close();
    resource.localPort = undefined;
    if (terminate) resource.worker.terminate();
  }
  function dispose(): void {
    if (disposed) return;
    // 先作废所有回调，再清理本人实例；迟到事件不能访问新实例。
    disposed = true;
    if (sweepTimer !== undefined) { timers.clearTimeout(sweepTimer); sweepTimer = undefined; }
    for (const resource of [...resources.values()]) cleanup(resource, true);
    protocol.manifest.clear(); protocol.pendingRecovery.length = 0;
    substrate = undefined;
    engineResource = undefined;
  }
  function fatal(detail: HostFailure): void {
    if (disposed) return;
    dispose();
    try { onFatalFailure(detail); }
    catch (error) {
      const report = new AggregateError([detail, error], "EngineSupervisor fatal callback 失败；全部本人资源已清理");
      if (reportError !== undefined) reportError(report);
      else if (typeof globalThis.reportError === "function") globalThis.reportError(report);
      else throw error;
    }
  }
  function spawnFailed(resource: Resource | undefined, identity: FacadeThreadIdentity, code: keyof typeof FacadeSpawnError, error: unknown, detail?: HostFailure): void {
    if (resource !== undefined) cleanup(resource, true);
    if (disposed) return;
    answerFail(arena(), identity.slot, identity.gen, FacadeSpawnError[code]);
    const original = detail === undefined ? failure(code, "spawn", error, identity) : detail;
    try { onSpawnFailure(original); }
    catch (callbackError) { fatal(failure("FACADE_SPAWN_REPORTER_ERROR", "onSpawnFailure", callbackError, { original })); }
  }
  function addListener(resource: Resource, target: EventTarget, type: string, listener: EventListener): void {
    target.addEventListener(type, listener);
    resource.listeners.push([target, type, listener]);
  }
  function evaluateWorkerError(resource: Resource, detail: HostFailure, eventMessage: string): void {
    if (resource.role === "child" && resource.identity !== undefined) {
      if (resource.ready && (eventMessage.includes(FACADE_BOOTSTRAP_REPORTER_FAILURE) || detail.message.includes(FACADE_BOOTSTRAP_REPORTER_FAILURE))) {
        fatal({ ...detail, code: FACADE_BOOTSTRAP_REPORTER_FAILURE }); return;
      }
      if (!resource.ready) { spawnFailed(resource, resource.identity, "SCRIPT_LOAD", detail, detail); return; }
      const view = arena();
      const join = Atomics.load(view.words, slotFieldOffset(resource.identity.slot, SLOT_JOIN_INDEX)) >>> 0;
      const outcome = writeDead(view, join, resource.identity.gen, "hardtrap", 1);
      if (outcome !== "written") return;
    }
    fatal(detail);
  }
  function workerError(resource: Resource, event: Event): void {
    if (disposed || !resources.has(resource.worker)) return;
    let originalFailure: HostFailure | undefined;
    let nativeEventMessage = "";
    try {
      const errorEvent = event as ErrorEvent;
      const rawMessage: unknown = errorEvent.message;
      nativeEventMessage = typeof rawMessage === "string" ? rawMessage : "";
      const rawError: unknown = errorEvent.error;
      const original = rawError === undefined || rawError === null ? new Error(nativeEventMessage || `Worker ${event.type}`) : rawError;
      const detail = failure(resource.role === "child" && !resource.ready ? "SCRIPT_LOAD" : "FACADE_WORKER_ERROR", resource.role, original, { nativeEventMessage });
      // engine 保留公开错误壳；分类器取得的原错误及 reporter 故障仍完整留在 cause。
      originalFailure = resource.role === "engine" && detail.code !== "FACADE_FAILURE_REPORTER_ERROR" ? {
        ...detail, code: "WASM_WORKER_SCRIPT", where: "worker-host",
        message: `WASM Worker 脚本加载或执行失败：${nativeEventMessage || detail.message}`,
        cause: detail.code === "FACADE_WORKER_ERROR" ? detail.cause : detail,
      } : detail;
      evaluateWorkerError(resource, originalFailure, nativeEventMessage);
    }
    catch (error) {
      if (disposed) throw error;
      fatal(failure("FACADE_WORKER_PROTOCOL_ERROR", resource.role, error, { originalFailure, nativeEventMessage }));
    }
  }
  function attach(worker: Worker, role: Resource["role"], identity?: FacadeThreadIdentity): Resource {
    if (disposed) { worker.terminate(); throw new Error("EngineSupervisor 已 dispose，不能登记 Worker"); }
    if (resources.has(worker)) throw new Error("EngineSupervisor 重复登记 Worker");
    // 构造成功即纳入强杀表，创建 Channel 失败也不能留下半开 Worker。
    const resource: Resource = { worker, role, identity: identity === undefined ? undefined : Object.freeze({ ...identity }), listeners: [], ready: role !== "child" };
    resources.set(worker, resource);
    try {
      if (role !== "ingress") {
        const channel = createChannel();
        Object.assign(resource, { port: channel.port1, localPort: channel.port2 });
        addListener(resource, channel.port1, "message", (event) => consume(resource, (event as MessageEvent).data, false));
        addListener(resource, channel.port1, "messageerror", () => {
          if (resources.has(worker)) fatal(failure("FACADE_CONTROL_ERROR", role, new Error("控制 Channel messageerror")));
        });
        channel.port1.start();
      }
      addListener(resource, worker, "error", (event) => workerError(resource, event));
      addListener(resource, worker, "messageerror", (event) => workerError(resource, event));
      if (role === "child") addListener(resource, worker, "message", (event) => consume(resource, (event as MessageEvent).data, true));
      return resource;
    } catch (error) { cleanup(resource, true); throw error; }
  }
  function identityMatches(resource: Resource, message: FacadeThreadIdentity): boolean {
    const id = resource.identity;
    return id !== undefined && id.workerId === message.workerId && id.slot === message.slot && id.gen === message.gen;
  }
  function validatePair(slot: number, gen: number): void {
    const view = arena();
    if (slot >= view.capacity || gen >= 2 ** view.genBits) throw new Error("facade slot/gen 超出当前 layout");
  }
  function spawn(slot: number, gen: number): void {
    validatePair(slot, gen);
    const view = arena();
    if (shuttingDown) {
      answerFail(view, slot, gen, FacadeSpawnError.SUPERVISOR_SHUTDOWN);
      onSpawnFailure(failure("SUPERVISOR_SHUTDOWN", "spawn", new Error("实例正在关停，spawn 未成立"), { slot, gen }));
      return;
    }
    if ((Atomics.load(view.words, slotWordOffset(slot)) >>> 0) !== encodeW(gen, SlotState.Req, view.genBits)) return;
    for (const resource of resources.values()) {
      if (resource.identity?.slot === slot && resource.identity.gen === gen) throw new Error("同代 facadeSpawn 重复请求");
    }
    const allocated = allocateFacadeWorkerId(nextWorkerId);
    nextWorkerId = allocated.next;
    const identity = Object.freeze({ workerId: allocated.workerId, slot, gen });
    let worker: Worker;
    try { worker = createChildWorker(); }
    catch (error) { spawnFailed(undefined, identity, "WORKER_CONSTRUCT", error); return; }
    let resource: Resource;
    try { resource = attach(worker, "child", identity); }
    catch (error) { spawnFailed(undefined, identity, "WORKER_CONSTRUCT", error); return; }
    if (substrate === undefined || resource.localPort === undefined) throw new Error("child bootstrap 资源不完整");
    const message: FacadeChildBootstrapMessage = { type: "facadeChildBootstrap", ...substrate, ...identity, controlPort: resource.localPort };
    resource.readyTimer = timers.setTimeout(() => {
      resource.readyTimer = undefined;
      if (!disposed && resources.has(worker)) spawnFailed(resource, identity, "SCRIPT_LOAD", new Error("child ready 超过 10000ms"));
    }, 10000);
    try {
      worker.postMessage(message, [resource.localPort]);
      resource.localPort = undefined;
    } catch (error) { spawnFailed(resource, identity, "INSTANTIATE", error); }
  }
  function dispatch(resource: Resource, message: FacadeSupervisorControlMessage, global: boolean): void {
    if (global && message.type !== "facadeBootstrapFailure") throw new Error("全局消息面只接受 facadeBootstrapFailure 错误出口");
    if (message.type === "facadeEngineSubstrate") {
      if (resource.role !== "engine" || resource.localPort !== undefined || resource.port === undefined) throw new Error("substrate 只能来自已接收首次 bootstrap 的 engine 自身控制端");
      installSubstrate(message);
      resource.port.postMessage({ type: "facadeEngineConfigured" });
      return;
    }
    if (message.type === "facadeBootstrapFailure") {
      if (resource.role !== "child") throw new Error("非 child 来源不能报告 child bootstrap failure");
      if (message.phase !== "validate" && !identityMatches(resource, message)) throw new Error("bootstrap failure 身份与控制端不一致");
      if (!resource.ready && (message.phase === "validate" || message.phase === "instantiate" || message.phase === "layout")) {
        if (resource.identity === undefined) throw new Error("child 缺少 bootstrap 身份");
        spawnFailed(resource, resource.identity, "INSTANTIATE", message.failure, message.failure);
      } else fatal(message.failure);
      return;
    }
    if (resource.role === "ingress") throw new Error("ingress 禁止 facade 控制消息");
    if (resource.role === "child" && !resource.ready && message.type !== "facadeChildReady") throw new Error("child ready 前不能发运行控制消息");
    switch (message.type) {
      case "facadeSpawn": checkpoint(); if (!disposed) spawn(message.slot, message.gen); return;
      case "facadeTerminateChild": {
        if (message.workerId < 1 || message.workerId >= nextWorkerId) throw new Error("terminate Worker id 不属于本实例已分配域");
        const target = [...resources.values()].find((candidate) => candidate.identity?.workerId === message.workerId);
        if (target === undefined) return;
        // Superseded/取消的真实资源先清理；已 OK 的生命周期由 exit/D5 裁决。
        if (target.ready) {
          fatal(failure("FACADE_ESTABLISHED_CHILD_TERMINATED", "terminate", new Error("已成立 child 被请求强杀；cleanup 无法再证明，本实例必须终止"), target.identity)); return;
        }
        cleanup(target, true); return;
      }
      case "facadeChildReady": {
        if (!identityMatches(resource, message) || resource.ready) throw new Error("child ready 身份不匹配或重复");
        if (resource.readyTimer !== undefined) { timers.clearTimeout(resource.readyTimer); resource.readyTimer = undefined; }
        registerChild(protocol, message.workerId, message.slot, message.gen);
        const outcome = answerOk(arena(), message.slot, message.gen, message.workerId, {
          onMessageEntry() {}, onAfterAcquire() {}, onTerminateLateChild() { cleanup(resource, true); },
        });
        if (outcome === "dropped-late") protocol.manifest.delete(message.workerId);
        else resource.ready = true;
        return;
      }
      case "facadeWorkerExit": {
        if (!identityMatches(resource, message)) throw new Error("child exit 身份与控制端不一致");
        cleanup(resource, false);
        const outcome = onWorkerExit(arena(), protocol, message.workerId, message.slot, message.gen);
        if (outcome === "pushed") protocol.manifest.delete(message.workerId);
        checkpoint(); return;
      }
    }
  }
  function consume(resource: Resource, value: unknown, global: boolean): boolean {
    if (disposed || !resources.has(resource.worker)) return true;
    try {
      const message = parseFacadeSupervisorControl(value);
      if (message === null) {
        if (!global) throw new Error("控制 Channel 收到非 facade 消息");
        return false;
      }
      dispatch(resource, message, global); return true;
    } catch (error) {
      if (disposed) throw error;
      fatal(failure("FACADE_PROTOCOL_ERROR", "control", error)); return true;
    }
  }
  function checkpoint(): void {
    if (disposed || substrate === undefined) return;
    const view = arena();
    for (const resource of [...resources.values()]) {
      const id = resource.identity;
      if (resource.ready || id === undefined) continue;
      if ((Atomics.load(view.words, slotWordOffset(id.slot)) >>> 0) !== encodeW(id.gen, SlotState.Req, view.genBits)) cleanup(resource, true);
    }
    for (const [id, entry] of protocol.manifest) {
      if (entry.status === "removed") {
        const outcome = evaluateOkRecovery(view, protocol, entry.slot, entry.gen);
        const raw = Atomics.load(view.words, slotWordOffset(entry.slot)) >>> 0;
        if (outcome === "pushed" || decodeGen(raw, view.genBits) !== entry.gen) protocol.manifest.delete(id);
      }
    }
    const recovered: Array<readonly [slot: number, gen: number]> = [];
    recoverPending(view, protocol, recovered);
    for (const [slot, gen] of recovered) {
      for (const [id, entry] of protocol.manifest) {
        if (entry.status === "removed" && entry.slot === slot && entry.gen === gen) protocol.manifest.delete(id);
      }
    }
    for (const resource of resources.values()) {
      const id = resource.identity;
      if (!resource.ready || id === undefined) continue;
      const joinIndex = Atomics.load(view.words, slotFieldOffset(id.slot, SLOT_JOIN_INDEX)) >>> 0;
      const raw = Atomics.load(view.words, joinWordOffset(joinIndex, view.capacity)) >>> 0;
      if (decodeGen(raw, view.genBits) !== id.gen) continue;
      const state = decodeJoinState(raw, view.genBits);
      if (state !== JoinState.Done && state !== JoinState.Consumed) continue;
      if (resource.doneObservedAt === undefined) resource.doneObservedAt = timers.now();
      if (timers.now() >= Math.min(resource.doneObservedAt + 10000, deadline)) {
        fatal(failure("FACADE_EXIT_ACK_TIMEOUT", "cleanup", new Error("DONE/CONSUMED 后未收到真实 workerExit"), id)); return;
      }
    }
  }
  function scheduleSweep(): void {
    if (disposed || substrate === undefined || sweepTimer !== undefined) return;
    let delay = 25;
    for (const resource of resources.values()) {
      if (resource.doneObservedAt !== undefined) delay = Math.min(delay, Math.max(0, Math.min(resource.doneObservedAt + 10000, deadline) - timers.now()));
    }
    sweepTimer = timers.setTimeout(() => {
      sweepTimer = undefined;
      try { checkpoint(); }
      catch (error) {
        if (disposed) throw error;
        fatal(failure("FACADE_SWEEP_ERROR", "checkpoint", error));
      }
      scheduleSweep();
    }, delay);
  }
  function tightenDeadline(until: number): void {
    if ((!Number.isFinite(until) && until !== Infinity) || until < 0) throw new Error("实例 deadline 必须为非负时间");
    deadline = Math.min(deadline, until);
    if (sweepTimer !== undefined) { timers.clearTimeout(sweepTimer); sweepTimer = undefined; }
    checkpoint(); scheduleSweep();
  }
  function installSubstrate(input: FacadeSubstrate): void {
    if (disposed || substrate !== undefined) throw new Error("facade substrate 只能装入一次活跃实例");
    const { module, memory, layout, diagnostics } = input;
    if (!(module instanceof WebAssembly.Module) || !(memory instanceof WebAssembly.Memory) || typeof diagnostics !== "boolean") throw new Error("facade substrate 必须含真实 module/shared memory/diagnostics");
    const buffer: unknown = memory.buffer;
    if (typeof SharedArrayBuffer !== "function" || !(buffer instanceof SharedArrayBuffer)) throw new Error("facade substrate 必须含 shared memory");
    for (const value of Object.values(layout)) if (!Number.isInteger(value) || value < 0 || value > 0xffffffff) throw new Error("facade layout 必须为 u32");
    if (layout.baseAddr % 4 !== 0 || layout.headerWords !== HEADER_WORDS || layout.slotStride !== SLOT_STRIDE || layout.joinStride !== JOIN_STRIDE || layout.capacity === 0 || layout.genBits < 1 || layout.genBits > GEN_BITS_DEFAULT) throw new Error("facade layout 与槽协议不一致");
    const length = layout.headerWords + layout.capacity * (layout.slotStride + layout.joinStride);
    if (layout.baseAddr + length * 4 > buffer.byteLength) throw new Error("facade layout 超出 memory");
    const view = new Int32Array(buffer, layout.baseAddr, length);
    if ((Atomics.load(view, CAPACITY) >>> 0) !== layout.capacity || (Atomics.load(view, GEN_BITS) >>> 0) !== layout.genBits) throw new Error("facade layout header 不匹配");
    substrate = Object.freeze({ module, memory, layout: Object.freeze({ ...layout }), diagnostics });
    scheduleSweep();
  }
  function assertConfigured(): void {
    if (disposed || substrate === undefined) throw new Error("EngineSupervisor 尚未 configured 或已 dispose");
  }
  engineResource = attach(options.engine, "engine");
  return {
    /** 端仅随首次 engine bootstrap 交付；发送失败先清全部真实资源再通知 fatal。 */
    bootstrapEngine(message: Readonly<Record<string, unknown>>): void {
      const engine = engineResource;
      if (disposed || engine === undefined || engine.localPort === undefined) throw new Error("engine bootstrap 只允许发送一次");
      try { engine.worker.postMessage({ ...message, controlPort: engine.localPort }, [engine.localPort]); engine.localPort = undefined; }
      catch (error) { fatal(failure("FACADE_ENGINE_BOOTSTRAP", "engine", error)); }
    },
    installSubstrate, assertConfigured,
    getSubstrate(): FacadeSubstrate {
      assertConfigured();
      if (substrate === undefined) throw new Error("EngineSupervisor substrate 尚未 configured");
      return substrate;
    },
    attachIngress(worker: Worker): void { assertConfigured(); attach(worker, "ingress"); },
    handleGlobalMessage(worker: Worker, value: unknown): boolean {
      const resource = resources.get(worker);
      return resource === undefined ? true : consume(resource, value, true);
    },
    beginShutdown(until?: number): void {
      if (until !== undefined && (!Number.isFinite(until) || until < 0)) throw new Error("实例 deadline 必须为有限非负时间");
      shuttingDown = true;
      for (const resource of [...resources.values()]) {
        if (!resource.ready && resource.identity !== undefined) spawnFailed(resource, resource.identity, "SUPERVISOR_SHUTDOWN", new Error("初始化在途 child 因实例关停而取消"));
      }
      tightenDeadline(until === undefined ? deadline : until);
    },
    setDeadline(until: number): void {
      if (!Number.isFinite(until) || until < 0) throw new Error("实例 deadline 必须为有限非负时间");
      tightenDeadline(until);
    },
    checkpoint, dispose,
    snapshot() {
      return { disposed, children: [...resources.values()].filter((resource) => resource.role === "child").length,
        workers: resources.size, ports: [...resources.values()].reduce((count, resource) => count + (resource.port === undefined ? 0 : 1) + (resource.localPort === undefined ? 0 : 1), 0),
        protocolEntries: protocol.manifest.size, pendingRecovery: protocol.pendingRecovery.length,
        timers: (sweepTimer === undefined ? 0 : 1) + [...resources.values()].filter((resource) => resource.readyTimer !== undefined).length };
    },
  };
}
