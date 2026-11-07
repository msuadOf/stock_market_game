import type { FacadeChildBootstrapMessage } from "./facade-child-bootstrap.ts";
import { FACADE_BOOTSTRAP_REPORTER_FAILURE } from "./facade-child-bootstrap.ts";
import type { FacadeSlotLayout, FacadeThreadIdentity } from "./facade-thread-binding.ts";
import type { HostFailure } from "./host-update.ts";
import { parseFacadeSupervisorControl, type FacadeSupervisorControlMessage } from "./facade-supervisor-control.ts";
import { parseHostFailure } from "./protocol-failure.ts";
import { classifyWasmFailure, describeWasmFailure } from "./wasm-failure.ts";
import { answerFail, answerOk, createSupervisorState, evaluateOkRecovery, onWorkerExit, recoverPending, registerChild, writeDead, type FacadeArenaView } from "./facade-supervisor.ts";
import { CAPACITY, GEN_BITS, GEN_BITS_DEFAULT, HEADER_WORDS, SLOT_STRIDE, JOIN_STRIDE, SLOT_JOIN_INDEX, SlotState, JoinState, decodeGen, decodeJoinState, encodeW, slotWordOffset, slotFieldOffset, joinWordOffset } from "./facade-slot-protocol.ts";
import { abortWasmWait, parseWasmWaitAbortLayout, type WasmWaitAbortLayout } from "./wasm-wait-abort.ts";
import { parseWasmWaitDeathLayout, watchWasmDeath, type WasmWaitDeathLayout } from "./wasm-wait-death.ts";
import type { WasmLifecycleObservation } from "./wasm-lifecycle-observation.ts";
import { readWasmTombstone } from "./wasm-tombstone.ts";
import type { WasmFinalRelease } from "./wasm-document-memory-owner.ts";

/** 只分配宿主错误载荷，不改变 Rust 的 WorkerFailed 判定。 */
export const FacadeSpawnError = Object.freeze({ WORKER_CONSTRUCT: 1, SCRIPT_LOAD: 2, INSTANTIATE: 3, SUPERVISOR_SHUTDOWN: 4, BOOTSTRAP_FAILURE: 5 });
export function allocateFacadeWorkerId(next: number): { workerId: number; next: number } {
  if (!Number.isInteger(next) || next < 1 || next > 0xfffffffe) throw new Error("facade Worker id 已耗尽或非法，禁止回绕");
  return { workerId: next, next: next + 1 };
}
export interface FacadePrimaryLayout { readonly engine: { readonly workerId: 0; readonly address: number }; readonly ingress: { readonly workerId: 4294967295; readonly address: number }; readonly death: WasmWaitDeathLayout }
export interface FacadeSubstrate {
  readonly module: WebAssembly.Module;
  readonly memory: WebAssembly.Memory;
  readonly layout: FacadeSlotLayout;
  readonly diagnostics: boolean;
  readonly waitAbort?: WasmWaitAbortLayout;
  readonly waitDeath?: WasmWaitDeathLayout;
  readonly primary?: FacadePrimaryLayout;
}
export interface SupervisorTimers {
  now(): number;
  setTimeout(callback: () => void, milliseconds: number): number;
  clearTimeout(id: number): void;
}
export interface EngineSupervisorOptions {
  readonly engine: Worker;
  readonly instanceId?: number;
  readonly createChildWorker: (name?: string) => Worker;
  readonly createChannel?: () => MessageChannel;
  readonly onFatalFailure: (failure: HostFailure) => void;
  readonly onFailurePending?: (failure: HostFailure) => void;
  readonly onSpawnFailure: (failure: HostFailure) => void;
  readonly reportError?: (error: Error) => void;
  /** 只交纯数据；构造途中未取得 Memory 事实时交 Error，不能编造零字节。 */
  readonly onLifecycleObservation?: (event: WasmLifecycleObservation) => void;
  readonly onFinalRelease?: (result: WasmFinalRelease | Error) => void;
  /** 仅用于确定性短 fixture；产品始终使用默认时钟及期限。 */
  readonly timers?: SupervisorTimers;
}
interface Resource {
  readonly worker: Worker;
  readonly role: "engine" | "child" | "ingress";
  readonly identity?: Readonly<FacadeThreadIdentity>;
  readonly recordAddr?: number;
  readonly port?: MessagePort;
  localPort?: MessagePort;
  readonly listeners: Array<readonly [EventTarget, string, EventListener]>;
  readyTimer?: number;
  ready: boolean;
  quiescent?: boolean;
  doneObservedAt?: number;
}

function parsePrimary(value: unknown, memory: WebAssembly.Memory): FacadePrimaryLayout {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("primary 布局必须为对象");
  const source = value as Record<string, unknown>;
  if (Object.keys(source).sort().join(",") !== "death,engine,ingress") throw new Error("primary 布局字段非法");
  function record(value: unknown, expected: number): { workerId: number; address: number } {
    if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error("primary 记录非法");
    const item = value as Record<string, unknown>;
    if (Object.keys(item).sort().join(",") !== "address,workerId" || item.workerId !== expected || typeof item.address !== "number" || !Number.isInteger(item.address) || item.address <= 0 || item.address % 4 !== 0 || item.address + 256 > memory.buffer.byteLength) throw new Error("primary 记录身份或地址非法");
    return { workerId: expected, address: item.address };
  }
  const engine = record(source.engine, 0), ingress = record(source.ingress, 0xffffffff);
  const death = parseWasmWaitDeathLayout(source.death, memory);
  if (engine.address < ingress.address + 256 && ingress.address < engine.address + 256 || [engine.address, ingress.address].some((address) => death.address < address + 256 && address < death.address + 4)) throw new Error("primary 记录与 DEAD 重叠");
  return Object.freeze({ engine: Object.freeze({ ...engine, workerId: 0 as const }), ingress: Object.freeze({ ...ingress, workerId: 0xffffffff as const }), death });
}

/** supervisor 所在的 control owner 持全部真实资源；manifest 只保存 W0 的 slot/gen 事实。 */
export function createEngineSupervisor(options: EngineSupervisorOptions) {
  const { createChildWorker, onFatalFailure, onSpawnFailure, reportError } = options;
  const timers: SupervisorTimers = options.timers === undefined ? {
    now: () => performance.now(),
    setTimeout: (callback, milliseconds) => globalThis.setTimeout(callback, milliseconds) as unknown as number,
    clearTimeout: (id) => globalThis.clearTimeout(id),
  } : options.timers;
  const createChannel = options.createChannel === undefined ? () => new MessageChannel() : options.createChannel;
  const resources = new Map<Worker, Resource>();
  const protocol = createSupervisorState();
  let substrate: FacadeSubstrate | undefined;
  let engineResource: Resource | undefined;
  let nextWorkerId = 1;
  const records: Array<{ address: number; identity: Readonly<FacadeThreadIdentity> }> = [];
  let disposed = false, shuttingDown = false;
  let deadline = Infinity;
  let sweepTimer: number | undefined;
  let cancelDeathWatch: (() => void) | undefined;
  let deathTimer: number | undefined;
  let deathReport: (() => void) | undefined;
  let pendingFailure: HostFailure | undefined;
  const subsequentFailures: HostFailure[] = [];
  let allocatedMemory: WebAssembly.Memory | undefined;
  let primary: FacadePrimaryLayout | undefined;
  let allocationStarted = false;
  let allocationConfirmedAbsent = false;
  let unprovenTermination = false;
  let requireWaitAbort = false;
  let finalRelease: WasmFinalRelease | Error | undefined;
  let drain: { readonly promise: Promise<void>; readonly resolve: () => void; readonly reject: (error: Error) => void } | undefined;

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
    const errors: unknown[] = [];
    function attempt(action: () => void): void { try { action(); } catch (error) { errors.push(error); } }
    if (resource.readyTimer !== undefined) { const timer = resource.readyTimer; resource.readyTimer = undefined; attempt(() => timers.clearTimeout(timer)); }
    for (const [target, type, listener] of resource.listeners) attempt(() => target.removeEventListener(type, listener));
    resource.listeners.length = 0;
    attempt(() => resource.port?.close());
    attempt(() => resource.localPort?.close());
    resource.localPort = undefined;
    if (terminate) {
      if (resource.quiescent !== true) unprovenTermination = true;
      // 即使 port/listener 清理失败也必须尝试 terminate；但 release 保留未知。
      attempt(() => resource.worker.terminate());
    }
    if (errors.length !== 0) throw new AggregateError(errors, "Worker 资源清理失败");
  }
  function dispose(): WasmFinalRelease | Error {
    if (disposed) {
      if (finalRelease === undefined) throw new Error("实例仍在同步释放中，禁止重入");
      return finalRelease;
    }
    // 先作废所有回调，再清理本人实例；迟到事件不能访问新实例。
    disposed = true;
    const pendingDrain = drain; drain = undefined;
    pendingDrain?.reject(new Error("实例已 dispose；强制清表不能证明真实 child exit"));
    const errors: unknown[] = [];
    if (deathTimer !== undefined) {
      const timer = deathTimer; deathTimer = undefined; deathReport = undefined;
      try { timers.clearTimeout(timer); } catch (error) { errors.push(error); }
    }
    if (cancelDeathWatch !== undefined) {
      const cancel = cancelDeathWatch; cancelDeathWatch = undefined;
      try { cancel(); } catch (error) { errors.push(error); }
    }
    if (sweepTimer !== undefined) {
      const timer = sweepTimer; sweepTimer = undefined;
      try { timers.clearTimeout(timer); } catch (error) { errors.push(error); }
    }
    const growthStopped = !unprovenTermination && [...resources.values()].every((resource) => resource.role !== "child" && resource.quiescent === true)
      && protocol.manifest.size === 0 && protocol.pendingRecovery.length === 0;
    for (const resource of [...resources.values()]) {
      try { cleanup(resource, true); }
      catch (error) { errors.push(error); }
    }
    let memory = allocatedMemory ?? substrate?.memory;
    let receipt: WasmFinalRelease | Error;
    try {
      if (errors.length !== 0) throw new AggregateError(errors, "整组 terminate 未全部成功，Memory 终值未知");
      if (memory === undefined) {
        if (allocationStarted && !allocationConfirmedAbsent) throw new Error("init 已开始但未取得 Memory 分配事实，禁止伪造 not-allocated 或零终值");
        receipt = { kind: "not-allocated" };
      } else {
        if (!growthStopped) throw new Error("缺少 cooperative quiescence/真实 child exit 证据；terminate 返回不能证明 Memory 已停止 grow");
        // cooperative quiescence 与真实 child exit 齐全后只读一次终值。
        const finalBytes = memory.buffer.byteLength;
        if (!Number.isSafeInteger(finalBytes) || finalBytes <= 0) throw new Error("Memory 终值不是正 safe integer bytes");
        receipt = { kind: "released", finalBytes, growthStopped: true, referencesDropped: true };
      }
    } catch (error) { receipt = error instanceof Error ? error : new Error("Memory 最终释放失败", { cause: error }); }
    protocol.manifest.clear(); protocol.pendingRecovery.length = 0;
    substrate = undefined;
    allocatedMemory = undefined;
    memory = undefined;
    engineResource = undefined;
    finalRelease = receipt;
    options.onFinalRelease?.(receipt);
    if (options.onLifecycleObservation !== undefined) options.onLifecycleObservation(receipt instanceof Error
      ? { type: "releaseUnknown", message: receipt.message }
      : receipt.kind === "released" ? { type: "finalRelease", kind: "released", finalBytes: receipt.finalBytes } : { type: "finalRelease", kind: "not-allocated" });
    return receipt;
  }
  function finalizeFatal(detail: HostFailure): void {
    if (disposed) return;
    const context = detail.context;
    let reported: HostFailure = { ...detail, context: context !== null && typeof context === "object" && !Array.isArray(context)
      ? { ...context, last_committed_tick: "unknown" }
      : context === undefined ? { last_committed_tick: "unknown" } : { original: context, last_committed_tick: "unknown" } };
    try { if (substrate?.waitAbort !== undefined) abortWasmWait(substrate.memory, substrate.waitAbort); }
    catch (error) { reported = { ...reported, cause: { primary: detail.cause, abort: failure("WASM_ABORT_WRITE", "fatal", error) } }; }
    try { dispose(); }
    catch (error) { reported = { ...reported, cause: { primary: reported.cause, releaseReporter: failure("WASM_RELEASE_REPORTER", "fatal", error) } }; }
    try { onFatalFailure(reported); }
    catch (error) {
      const report = new AggregateError([detail, error], "EngineSupervisor fatal callback 失败；全部本人资源已清理");
      if (reportError !== undefined) reportError(report);
      else if (typeof globalThis.reportError === "function") globalThis.reportError(report);
      else throw error;
    }
  }
  function fatal(detail: HostFailure): void {
    if (disposed) return;
    if (substrate?.waitDeath === undefined && primary === undefined) { finalizeFatal(detail); return; }
    if (pendingFailure !== undefined) { subsequentFailures.push(detail); return; }
    pendingFailure = detail;
    shuttingDown = true;
    try { if (substrate?.waitAbort !== undefined) abortWasmWait(substrate.memory, substrate.waitAbort); }
    catch (error) { subsequentFailures.push(failure("WASM_ABORT_WRITE", "failurePending", error)); }
    try {
      if (options.onFailurePending === undefined) throw new Error("waitDeath 需要 failurePending 控制端");
      options.onFailurePending(detail);
      schedulePendingFailure();
    } catch (error) { finalizeFatal({ ...detail, cause: { primary: detail.cause, pendingReporter: describeWasmFailure(error) } }); }
  }
  function schedulePendingFailure(): void {
    if (pendingFailure === undefined || disposed || deadline === Infinity) return;
    if (deathTimer !== undefined) timers.clearTimeout(deathTimer);
    deathReport = () => {
      deathTimer = undefined; deathReport = undefined;
      if (disposed || pendingFailure === undefined) return;
      const allRecords = [...(primary === undefined ? [] : [
        { address: primary.engine.address, identity: { workerId: 0, role: "engine" } },
        { address: primary.ingress.address, identity: { workerId: 0xffffffff, role: "ingress" } },
      ]), ...records];
      const snapshots = allRecords.map(({ address, identity }) => {
        try {
          const memory = substrate?.memory ?? allocatedMemory;
          if (memory === undefined) throw new Error("facade Memory 尚未登记");
          return { identity, tombstone: readWasmTombstone(memory, address, identity.workerId) };
        } catch (error) { return { identity, tombstoneReadError: describeWasmFailure(error) }; }
      });
      finalizeFatal({ ...pendingFailure, context: { original: pendingFailure.context, records: snapshots, subsequentFailures, incomplete: true } });
    };
    deathTimer = timers.setTimeout(deathReport, Math.max(0, deadline - timers.now()));
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
  function withTombstone(resource: Resource, detail: HostFailure): HostFailure {
    if (resource.recordAddr === undefined || resource.identity === undefined || substrate === undefined) return detail;
    try {
      const tombstone = readWasmTombstone(substrate.memory, resource.recordAddr, resource.identity.workerId);
      return { ...detail, context: { original: detail.context, tombstone } };
    } catch (error) {
      return { ...detail, context: { original: detail.context, tombstoneReadError: describeWasmFailure(error) } };
    }
  }
  function evaluateWorkerError(resource: Resource, detail: HostFailure, eventMessage: string): void {
    if (resource.role === "child" && resource.identity !== undefined) {
      if (resource.ready && (eventMessage.includes(FACADE_BOOTSTRAP_REPORTER_FAILURE) || detail.message.includes(FACADE_BOOTSTRAP_REPORTER_FAILURE))) {
        fatal(withTombstone(resource, { ...detail, code: FACADE_BOOTSTRAP_REPORTER_FAILURE })); return;
      }
      if (!resource.ready) { spawnFailed(resource, resource.identity, "SCRIPT_LOAD", detail, detail); return; }
      const view = arena();
      const join = Atomics.load(view.words, slotFieldOffset(resource.identity.slot, SLOT_JOIN_INDEX)) >>> 0;
      const outcome = writeDead(view, join, resource.identity.gen, "hardtrap", 1);
      if (outcome !== "written" && outcome !== "lost-to-dead") return;
    }
    fatal(resource.role === "child" ? withTombstone(resource, detail) : detail);
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
  function spawn(slot: number, gen: number, runtimeName: string, recordAddr?: number): void {
    validatePair(slot, gen);
    const view = arena();
    if (shuttingDown) {
      answerFail(view, slot, gen, FacadeSpawnError.SUPERVISOR_SHUTDOWN);
      onSpawnFailure(failure("SUPERVISOR_SHUTDOWN", "spawn", new Error("实例正在关停，spawn 未成立"), { slot, gen }));
      return;
    }
    if ((Atomics.load(view.words, slotWordOffset(slot)) >>> 0) !== encodeW(gen, SlotState.Req, view.genBits)) return;
    if (recordAddr !== undefined) {
      if (substrate === undefined) throw new Error("facade substrate 尚未装入");
      const { layout, memory } = substrate;
      const arenaEnd = layout.baseAddr + (layout.headerWords + layout.capacity * (layout.slotStride + layout.joinStride)) * 4;
      if (recordAddr === 0 || recordAddr % 4 !== 0 || recordAddr + 256 > memory.buffer.byteLength ||
          (recordAddr < arenaEnd && recordAddr + 256 > layout.baseAddr) ||
          (primary !== undefined && [primary.engine.address, primary.ingress.address].some((address) => recordAddr < address + 256 && address < recordAddr + 256)) ||
          (substrate.waitDeath !== undefined && recordAddr < substrate.waitDeath.address + 4 && substrate.waitDeath.address < recordAddr + 256) ||
          (substrate.waitAbort !== undefined && recordAddr <= substrate.waitAbort.address && substrate.waitAbort.address < recordAddr + 256) ||
          records.some(({ address }) => recordAddr < address + 256 && address < recordAddr + 256)) {
        throw new Error("facadeSpawnV2 记录地址非法、重叠或已登记");
      }
    }
    for (const resource of resources.values()) {
      if (resource.identity?.slot === slot && resource.identity.gen === gen) throw new Error("同代 facadeSpawn 重复请求");
    }
    const allocated = allocateFacadeWorkerId(nextWorkerId);
    nextWorkerId = allocated.next;
    const identity = Object.freeze({ workerId: allocated.workerId, slot, gen });
    if (recordAddr !== undefined) records.push({ address: recordAddr, identity });
    let worker: Worker;
    const childName = `stock-game:${options.instanceId ?? 0}:child:${allocated.workerId}:${slot}:${gen}`;
    try { worker = createChildWorker(childName); }
    catch (error) { spawnFailed(undefined, identity, "WORKER_CONSTRUCT", error); return; }
    let resource: Resource;
    try { resource = attach(worker, "child", identity); }
    catch (error) { spawnFailed(undefined, identity, "WORKER_CONSTRUCT", error); return; }
    if (recordAddr !== undefined) Object.assign(resource, { recordAddr });
    if (substrate === undefined || resource.localPort === undefined) throw new Error("child bootstrap 资源不完整");
    options.onLifecycleObservation?.({ type: "childSpawn", ...identity, name: childName, runtimeName });
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
    if (message.type === "facadeEngineMemory") {
      if (resource.role !== "engine" || resource.localPort !== undefined || resource.port === undefined || allocatedMemory !== undefined || substrate !== undefined) throw new Error("Memory 只能由首次 engine bootstrap 的控制端登记一次");
      allocatedMemory = message.memory;
      resource.port.postMessage({ type: "facadeEngineMemoryRegistered", allocationId: 1 });
      options.onLifecycleObservation?.({ type: "memoryRegistered", allocationId: 1, role: "engine" });
      return;
    }
    if (message.type === "facadeEngineDeath") {
      if (resource.role !== "engine" || resource.port === undefined || allocatedMemory === undefined || substrate !== undefined || primary !== undefined) throw new Error("primary 只能在首次 Memory 登记后配置");
      primary = parsePrimary(message.primary, allocatedMemory);
      cancelDeathWatch = watchWasmDeath(allocatedMemory, primary.death, () => {
        if (!disposed) fatal({ code: "WASM_WORKER_DEATH", where: "engine-supervisor.death", message: "WASM Worker 报告死亡；诊断记录可能尚未发布", recoverable: false, recoveryActions: ["请保留错误详情并反馈"] });
      }, (error) => { if (!disposed) fatal(failure("WASM_DEATH_WATCH_ERROR", "death", error)); });
      resource.port.postMessage({ type: "facadeEngineDeathRegistered", allocationId: 1 });
      return;
    }
    if (message.type === "facadeEngineConfiguration") {
      if (resource.role !== "engine" || resource.localPort !== undefined || resource.port === undefined || allocatedMemory === undefined) throw new Error("配置必须关联本控制端已登记的唯一 Memory");
      if (requireWaitAbort && message.waitAbort === undefined) throw new Error("runtime true 缺少真实 wait abort 布局");
      if (requireWaitAbort && primary === undefined) throw new Error("runtime true 缺少提前登记 primary 布局");
      const waitAbort = message.waitAbort === undefined ? undefined : parseWasmWaitAbortLayout(message.waitAbort, allocatedMemory);
      installSubstrate({ module: message.module, memory: allocatedMemory, layout: message.layout, diagnostics: message.diagnostics, ...(waitAbort === undefined ? {} : { waitAbort }), ...(message.waitDeath === undefined ? {} : { waitDeath: message.waitDeath as WasmWaitDeathLayout }) });
      resource.port.postMessage({ type: "facadeEngineConfigured" });
      options.onLifecycleObservation?.({ type: "allocationConfigured", allocationId: 1, role: "engine", workerId: null, slot: null, gen: null });
      return;
    }
    if (message.type === "facadeEngineSubstrate") {
      if (resource.role !== "engine" || resource.localPort !== undefined || resource.port === undefined) throw new Error("substrate 只能来自已接收首次 bootstrap 的 engine 自身控制端");
      if (allocatedMemory !== undefined || requireWaitAbort) throw new Error("已登记 Memory 或 runtime true 必须使用 allocation configuration，禁止重传 Memory");
      installSubstrate(message);
      resource.port.postMessage({ type: "facadeEngineConfigured" });
      options.onLifecycleObservation?.({ type: "allocationConfigured", allocationId: 1, role: "engine", workerId: null, slot: null, gen: null });
      return;
    }
    if (message.type === "facadeBootstrapFailure") {
      if (resource.role !== "child") throw new Error("非 child 来源不能报告 child bootstrap failure");
      if (message.phase !== "validate" && !identityMatches(resource, message)) throw new Error("bootstrap failure 身份与控制端不一致");
      if (!resource.ready && (message.phase === "validate" || message.phase === "instantiate" || message.phase === "layout")) {
        if (resource.identity === undefined) throw new Error("child 缺少 bootstrap 身份");
        spawnFailed(resource, resource.identity, "INSTANTIATE", message.failure, message.failure);
      } else {
        // 已成立的 child 可能在 trampoline 中 trap；先唤醒 join，再废弃实例。
        // 有记录时保留诊断；未发布时只能按 hardtrap 表达。
        if (!resource.ready && resource.identity !== undefined) {
          try {
            // 全局错误出口可抢先于控制端的 ready；此时 spawn 尚未成立。
            answerFail(arena(), resource.identity.slot, resource.identity.gen, FacadeSpawnError.BOOTSTRAP_FAILURE);
          } catch (error) {
            fatal(failure("FACADE_WORKER_PROTOCOL_ERROR", resource.role, error, { originalFailure: message.failure }));
            return;
          }
        } else if (resource.ready && resource.identity !== undefined) {
          try {
            const view = arena();
            const join = Atomics.load(view.words, slotFieldOffset(resource.identity.slot, SLOT_JOIN_INDEX)) >>> 0;
            writeDead(view, join, resource.identity.gen, "hardtrap", 1);
          } catch (error) {
            fatal(failure("FACADE_WORKER_PROTOCOL_ERROR", resource.role, error, { originalFailure: message.failure }));
            return;
          }
        }
        fatal(withTombstone(resource, message.failure));
      }
      return;
    }
    if (resource.role === "ingress") throw new Error("ingress 禁止 facade 控制消息");
    if (resource.role === "child" && !resource.ready && message.type !== "facadeChildReady") throw new Error("child ready 前不能发运行控制消息");
    switch (message.type) {
      case "facadeSpawn": checkpoint(); if (!disposed) spawn(message.slot, message.gen, message.name); return;
      case "facadeSpawnV2": checkpoint(); if (!disposed) spawn(message.slot, message.gen, message.name, message.recordAddr); return;
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
        else {
          resource.ready = true;
          if (allocatedMemory !== undefined) options.onLifecycleObservation?.({ type: "allocationConfigured", allocationId: 1, role: "child", workerId: message.workerId, slot: message.slot, gen: message.gen });
        }
        return;
      }
      case "facadeWorkerExit": {
        if (!identityMatches(resource, message)) throw new Error("child exit 身份与控制端不一致");
        cleanup(resource, false);
        const outcome = onWorkerExit(arena(), protocol, message.workerId, message.slot, message.gen);
        if (outcome === "pushed") protocol.manifest.delete(message.workerId);
        checkpoint();
        options.onLifecycleObservation?.({ type: "childExit", workerId: message.workerId, slot: message.slot, gen: message.gen, ...lifecycleCounts() });
        return;
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
  function lifecycleCounts() {
    return { activeChildren: [...resources.values()].filter((resource) => resource.role === "child").length, manifestEntries: protocol.manifest.size, pendingRecovery: protocol.pendingRecovery.length };
  }
  function checkpoint(): void {
    if (disposed) return;
    if (substrate === undefined) {
      // 初始化尚未配置 arena 时也能证明实际资源表为空；owner 仍须另等 engine/ingress 的真实 quiescence。
      if (drain !== undefined && ![...resources.values()].some((resource) => resource.role === "child") && protocol.manifest.size === 0 && protocol.pendingRecovery.length === 0) {
        const pending = drain;
        options.onLifecycleObservation?.({ type: "childrenDrained", ...lifecycleCounts() });
        drain = undefined; pending.resolve();
      }
      return;
    }
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
    if (drain !== undefined) {
      if (![...resources.values()].some((resource) => resource.role === "child") && protocol.manifest.size === 0 && protocol.pendingRecovery.length === 0) {
        const pending = drain;
        options.onLifecycleObservation?.({ type: "childrenDrained", ...lifecycleCounts() });
        drain = undefined; pending.resolve();
      } else if (timers.now() >= deadline) {
        fatal(failure("WASM_SHUTDOWN_EXIT_TIMEOUT", "shutdown", new Error("正常停止期限内未收到全部真实 child exit/槽回收")));
      }
    }
  }
  function scheduleSweep(): void {
    if (disposed || substrate === undefined || sweepTimer !== undefined) return;
    let delay = 25;
    if (drain !== undefined) delay = Math.min(delay, Math.max(0, deadline - timers.now()));
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
    schedulePendingFailure();
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
    if (Object.keys(layout).sort().join(",") !== "baseAddr,capacity,genBits,headerWords,joinStride,slotStride") throw new Error("facade layout 字段不完整或有未知字段");
    const waitAbort = input.waitAbort === undefined ? undefined : parseWasmWaitAbortLayout(input.waitAbort, memory);
    const waitDeath = input.waitDeath === undefined ? primary?.death : parseWasmWaitDeathLayout(input.waitDeath, memory);
    if (waitDeath !== undefined) {
      const overlaps = (start: number, length: number) => waitDeath.address < start + length && start < waitDeath.address + 4;
      if (overlaps(layout.baseAddr, length * 4) || (waitAbort !== undefined && overlaps(waitAbort.address, 1))) throw new Error("wasm DEAD 布局与 arena 或 abort 重叠");
    }
    substrate = Object.freeze({ module, memory, layout: Object.freeze({ ...layout }), diagnostics, ...(waitAbort === undefined ? {} : { waitAbort }), ...(waitDeath === undefined ? {} : { waitDeath }) });
    if (primary !== undefined && (waitDeath === undefined || waitDeath.address !== primary.death.address)) throw new Error("配置 DEAD 地址与提前登记不一致");
    if (primary !== undefined && (primary.engine.address < layout.baseAddr + length * 4 && layout.baseAddr < primary.engine.address + 256 || primary.ingress.address < layout.baseAddr + length * 4 && layout.baseAddr < primary.ingress.address + 256)) throw new Error("primary 记录与 arena 重叠");
    if (primary !== undefined && waitAbort !== undefined && [primary.engine.address, primary.ingress.address].some((address) => address <= waitAbort.address && waitAbort.address < address + 256)) throw new Error("primary 记录与 abort 重叠");
    if (primary !== undefined) substrate = Object.freeze({ ...substrate, primary });
    if (waitDeath !== undefined && primary === undefined) cancelDeathWatch = watchWasmDeath(memory, waitDeath, () => {
      if (!disposed) fatal({ code: "WASM_WORKER_DEATH", where: "engine-supervisor.death", message: "WASM Worker 报告死亡；诊断记录可能尚未发布", recoverable: false, recoveryActions: ["请保留错误详情并反馈"] });
    }, (error) => { if (!disposed) fatal(failure("WASM_DEATH_WATCH_ERROR", "death", error)); });
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
      allocationStarted = true;
      requireWaitAbort = message.runtimeOverride === true;
      try { engine.worker.postMessage({ ...message, controlPort: engine.localPort }, [engine.localPort]); engine.localPort = undefined; }
      catch (error) { fatal(failure("FACADE_ENGINE_BOOTSTRAP", "engine", error)); }
    },
    bootstrapEngineTransport(dataPort: MessagePort, instanceId: number, runtimeOverride = false): void {
      const engine = engineResource;
      if (disposed || engine === undefined || engine.localPort === undefined) throw new Error("engine transport bootstrap 只允许发送一次");
      allocationStarted = true;
      requireWaitAbort = runtimeOverride;
      try {
        engine.worker.postMessage({ type: "bindEngineTransport", instanceId, dataPort, controlPort: engine.localPort }, [dataPort, engine.localPort]);
        engine.localPort = undefined;
      } catch (error) { fatal(failure("FACADE_ENGINE_BOOTSTRAP", "engine", error)); }
    },
    finishFailureAtDeadline(detail: HostFailure): void {
      if (!Number.isFinite(deadline) || timers.now() < deadline) throw new Error("failure deadline 尚未到期，不能提前终止 writer");
      if (disposed) return;
      fatal(parseHostFailure(detail, "engine-supervisor.deadline"));
      if (!disposed && deathReport !== undefined) {
        const report = deathReport;
        if (deathTimer !== undefined) timers.clearTimeout(deathTimer);
        report();
      }
    },
    noteFailureDuringPending(detail: HostFailure): void {
      if (disposed || pendingFailure === undefined) throw new Error("只有 failurePending 期间能追加关停错误");
      subsequentFailures.push(parseHostFailure(detail, "engine-supervisor.failurePending"));
    },
    installSubstrate, assertConfigured,
    getSubstrate(): FacadeSubstrate {
      assertConfigured();
      if (substrate === undefined) throw new Error("EngineSupervisor substrate 尚未 configured");
      return substrate;
    },
    attachIngress(worker: Worker): void { assertConfigured(); attach(worker, "ingress"); },
    observeIngressConfigured(worker: Worker): void {
      const resource = resources.get(worker);
      if (disposed || resource === undefined || resource.role !== "ingress") throw new Error("ingress configured 观察必须属于本人活跃 ingress");
      if (allocatedMemory !== undefined) options.onLifecycleObservation?.({ type: "allocationConfigured", allocationId: 1, role: "ingress", workerId: null, slot: null, gen: null });
    },
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
    waitForChildrenExit(until: number): Promise<void> {
      if (disposed) return Promise.reject(new Error("实例已经 dispose，不能证明真实 child exit"));
      if (!Number.isFinite(until) || until < 0) return Promise.reject(new Error("实例 deadline 必须为有限非负时间"));
      if (drain !== undefined) { tightenDeadline(until); return drain.promise; }
      let resolve!: () => void, reject!: (error: Error) => void;
      const promise = new Promise<void>((done, failed) => { resolve = done; reject = failed; });
      drain = { promise, resolve, reject };
      tightenDeadline(until);
      return promise;
    },
    setDeadline(until: number): void {
      if (!Number.isFinite(until) || until < 0) throw new Error("实例 deadline 必须为有限非负时间");
      tightenDeadline(until);
    },
    markEngineUnallocated(worker: Worker): void {
      const resource = resources.get(worker);
      if (disposed || resource === undefined || resource.role !== "engine" || resource.quiescent === true || allocatedMemory !== undefined || substrate !== undefined || allocationConfirmedAbsent) throw new Error("pre-Memory 未分配证明与实例阶段或已有分配事实矛盾");
      allocationConfirmedAbsent = true;
      resource.quiescent = true;
      options.onLifecycleObservation?.({ type: "quiescent", role: "engine", ...lifecycleCounts() });
    },
    markQuiescent(worker: Worker): void {
      const resource = resources.get(worker);
      if (disposed || resource === undefined || resource.role === "child" || resource.quiescent === true) throw new Error("quiescence 必须来自活跃 engine/ingress 的一次真实回复");
      resource.quiescent = true;
      options.onLifecycleObservation?.({ type: "quiescent", role: resource.role, ...lifecycleCounts() });
    },
    checkpoint, dispose, discard: fatal,
    snapshot() {
      return { disposed, children: [...resources.values()].filter((resource) => resource.role === "child").length,
        workers: resources.size, ports: [...resources.values()].reduce((count, resource) => count + (resource.port === undefined ? 0 : 1) + (resource.localPort === undefined ? 0 : 1), 0),
        protocolEntries: protocol.manifest.size, pendingRecovery: protocol.pendingRecovery.length,
        timers: (sweepTimer === undefined ? 0 : 1) + (deathTimer === undefined ? 0 : 1) + [...resources.values()].filter((resource) => resource.readyTimer !== undefined).length };
    },
  };
}
