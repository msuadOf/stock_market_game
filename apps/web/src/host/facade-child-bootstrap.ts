import type { FacadeThreadConfiguration, FacadeThreadIdentity, FacadeControlMessage, FacadeSlotLayout } from "./facade-thread-binding.ts";
import type { FacadeArenaView } from "./facade-supervisor.ts";
import type { HostFailure } from "./host-update.ts";
import type { FacadeSyncInitializer } from "./facade-wasm-module.ts";
import { ChildShutdown, type WorkerExitMessage } from "./facade-child-shutdown.ts";
import { instantiateFacadeModule } from "./facade-wasm-module.ts";
import { GEN_BITS_DEFAULT, HEADER_WORDS, JOIN_STRIDE, SLOT_STRIDE } from "./facade-slot-protocol.ts";
import { classifyWasmFailure, describeReadFailureFields } from "./wasm-failure.ts";

export interface FacadeChildBootstrapMessage extends FacadeThreadIdentity {
  readonly type: "facadeChildBootstrap";
  readonly module: WebAssembly.Module;
  readonly memory: WebAssembly.Memory;
  readonly layout: FacadeSlotLayout;
  readonly diagnostics: boolean;
  readonly controlPort: MessagePort;
}
export interface FacadeChildReadyMessage extends FacadeThreadIdentity { readonly type: "facadeChildReady" }
/** ErrorEvent.error 可为 null；message 中仍保留此标记供 DONE 后的独立故障入口识别。 */
export const FACADE_BOOTSTRAP_REPORTER_FAILURE = "FACADE_BOOTSTRAP_REPORTER_FAILURE";
export type FacadeBootstrapPhase = "validate" | "instantiate" | "layout" | "trampoline" | "cleanup";
export type FacadeBootstrapFailureMessage =
  | { readonly type: "facadeBootstrapFailure"; readonly phase: "validate"; readonly failure: HostFailure }
  | (FacadeThreadIdentity & { readonly type: "facadeBootstrapFailure"; readonly phase: Exclude<FacadeBootstrapPhase, "validate">; readonly failure: HostFailure });
export type FacadeChildControlMessage = FacadeControlMessage | FacadeChildReadyMessage | WorkerExitMessage | FacadeBootstrapFailureMessage;
export interface FacadeChildGlue extends FacadeSyncInitializer {
  readonly facade_child_trampoline: (slot: number, gen: number, workerId: number) => number;
  readonly facade_slot_layout: () => FacadeSlotLayout;
}
export interface FacadeChildEnvironment {
  readonly bindFacadeThreadEnvironment: (input: FacadeThreadConfiguration) => void;
  readonly installFacadeSlotLayout: (input: FacadeSlotLayout) => void;
  readonly getArenaView: () => FacadeArenaView;
  readonly requireExitIntent: () => Readonly<FacadeThreadIdentity>;
}
export interface FacadeChildBootstrapDependencies {
  readonly environment: FacadeChildEnvironment;
  readonly loadGlue: (diagnostics: boolean) => Promise<FacadeChildGlue>;
  readonly postControl: (message: FacadeChildControlMessage, input: FacadeChildBootstrapMessage) => void;
  readonly postValidationFailure: (message: FacadeBootstrapFailureMessage) => void;
  readonly reportError: (error: Error) => void;
  readonly close: () => void;
}
function record(value: unknown, where: string): Record<string, unknown> {
  if (value === null || typeof value !== "object") throw new Error(`${where} 必须是对象`);
  return value as Record<string, unknown>;
}
function u32(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isInteger(value) || value < 0 || value > 0xffffffff) throw new Error(`${field} 必须为 u32`);
  return value;
}
export function parseFacadeChildBootstrap(value: unknown): FacadeChildBootstrapMessage {
  const source = record(value, "facadeChildBootstrap");
  if (source.type !== "facadeChildBootstrap") throw new Error("子 Worker 只接受 facadeChildBootstrap");
  if (!(source.module instanceof WebAssembly.Module)) throw new Error("bootstrap module 必须为 WebAssembly.Module");
  if (!(source.memory instanceof WebAssembly.Memory) || !((source.memory.buffer as ArrayBufferLike) instanceof SharedArrayBuffer)) throw new Error("bootstrap memory 必须为共享 WebAssembly.Memory");
  if (!(source.controlPort instanceof MessagePort)) throw new Error("bootstrap controlPort 必须为 MessagePort");
  if (typeof source.diagnostics !== "boolean") throw new Error("bootstrap diagnostics 必须为 boolean");
  const info = record(source.layout, "bootstrap layout");
  const layout: FacadeSlotLayout = Object.freeze({
    baseAddr: u32(info.baseAddr, "layout.baseAddr"), headerWords: u32(info.headerWords, "layout.headerWords"),
    slotStride: u32(info.slotStride, "layout.slotStride"), joinStride: u32(info.joinStride, "layout.joinStride"),
    capacity: u32(info.capacity, "layout.capacity"), genBits: u32(info.genBits, "layout.genBits"),
  });
  if (layout.baseAddr % 4 !== 0 || layout.headerWords !== HEADER_WORDS || layout.slotStride !== SLOT_STRIDE || layout.joinStride !== JOIN_STRIDE || layout.capacity === 0 || layout.genBits < 1 || layout.genBits > GEN_BITS_DEFAULT) throw new Error("bootstrap layout 与槽协议不一致");
  if (layout.baseAddr + 4 * (layout.headerWords + layout.capacity * (layout.slotStride + layout.joinStride)) > source.memory.buffer.byteLength) throw new Error("bootstrap layout 超出 memory");
  const workerId = u32(source.workerId, "workerId"), slot = u32(source.slot, "slot"), gen = u32(source.gen, "gen");
  if (slot >= layout.capacity || gen >= 2 ** layout.genBits) throw new Error("bootstrap 身份超出 layout 边界");
  return Object.freeze({ type: "facadeChildBootstrap", module: source.module, memory: source.memory, layout, workerId, slot, gen, diagnostics: source.diagnostics, controlPort: source.controlPort });
}

/** 只读取一次错误核心字段；原错误可能是带 getter 的未知对象。 */
function snapshotFailure(error: unknown): { message: string; cause?: Readonly<Record<string, unknown>> } {
  const classified = classifyWasmFailure(error);
  if (classified.kind === "structured") return { message: classified.message, cause: { code: classified.code, message: classified.message } };
  if (classified.kind === "access-error") return { message: `读取错误字段 ${classified.field} 失败：${classified.reason}${describeReadFailureFields(classified.cause)}`, cause: classified.cause };
  return { message: classified.description.trim().length === 0 ? "bootstrap 原错误没有可读的非空 message" : classified.description, cause: classified.cause };
}

/** 只有此入口拥有本 child 的生命周期；加载期间的重复输入也不可重入 Rust。 */
export function createFacadeChildBootstrap(dependencies: FacadeChildBootstrapDependencies): { handleMessage(value: unknown): Promise<void>; isFinished(): boolean } {
  let started = false, finished = false;
  let input: FacadeChildBootstrapMessage | undefined;
  let phase: FacadeBootstrapPhase = "validate";
  const closedPorts = new Set<MessagePort>();
  let scopeClosed = false;
  function closeScope(): void {
    if (!scopeClosed) { scopeClosed = true; dependencies.close(); }
  }
  function closePort(port: MessagePort): void {
    if (!closedPorts.has(port)) { closedPorts.add(port); port.close(); }
  }
  function closeReceivedPort(value: unknown): void {
    try {
      if (value !== null && typeof value === "object") {
        // 消息来自 structured clone；负测中的 accessor 不应在清理时再次执行。
        const descriptor = Object.getOwnPropertyDescriptor(value, "controlPort");
        if (descriptor !== undefined && !Object.hasOwn(descriptor, "value")) throw new Error("bootstrap controlPort 是 accessor，无法安全清理");
        const port: unknown = descriptor?.value;
        if (port instanceof MessagePort && port !== input?.controlPort) closePort(port);
      }
    } catch (error) { dependencies.reportError(new Error(`关闭收到的 bootstrap port 失败：${snapshotFailure(error).message}`)); }
  }
  function fail(error: unknown): void {
    finished = true;
    const snapshot = snapshotFailure(error);
    const original = new Error(snapshot.message);
    const failure: HostFailure = { code: "FACADE_BOOTSTRAP_FAILURE", where: `facade-child-bootstrap.${phase}`, ...snapshot, recoverable: false, recoveryActions: ["请保留错误详情并反馈；本实例需要终止"] };
    const message: FacadeBootstrapFailureMessage = phase === "validate" || input === undefined
      ? { type: "facadeBootstrapFailure", phase: "validate", failure }
      : { type: "facadeBootstrapFailure", phase, workerId: input.workerId, slot: input.slot, gen: input.gen, failure };
    try {
      if (input === undefined) dependencies.postValidationFailure(message);
      else dependencies.postControl(message, input);
    } catch (postError) {
      const transportError = snapshotFailure(postError);
      try {
        dependencies.postValidationFailure({ ...message, failure: { ...failure, context: { controlPostError: transportError } } });
      } catch (globalPostError) {
        const globalError = snapshotFailure(globalPostError);
        dependencies.reportError(new AggregateError([original, new Error(transportError.message), new Error(globalError.message)], `[${FACADE_BOOTSTRAP_REPORTER_FAILURE}] ${failure.code}（${failure.where}）：${failure.message}；控制/全局消息均无法投递；控制：${transportError.message}；全局：${globalError.message}`));
      }
    } finally {
      if (input !== undefined) closePort(input.controlPort);
      closeScope();
    }
  }
  async function handleMessage(value: unknown): Promise<void> {
    if (finished) { closeReceivedPort(value); return; }
    if (started) { closeReceivedPort(value); fail(new Error("facade bootstrap 重复输入；只允许启动一次")); return; }
    started = true;
    try {
      input = parseFacadeChildBootstrap(value);
      const current = input;
      phase = "instantiate";
      dependencies.environment.bindFacadeThreadEnvironment({ memory: current.memory, role: "child", bootstrap: current, postControl: (message) => dependencies.postControl(message, current) });
      const glue = await dependencies.loadGlue(current.diagnostics);
      if (finished) return;
      const raw = instantiateFacadeModule(glue, current.module, current.memory);
      phase = "layout";
      const actual = glue.facade_slot_layout();
      for (const key of Object.keys(current.layout) as (keyof FacadeSlotLayout)[]) if (actual[key] !== current.layout[key]) throw new Error(`facade layout.${key} 与实际 Rust 导出不一致`);
      dependencies.environment.installFacadeSlotLayout(current.layout);
      dependencies.environment.getArenaView();
      const identity = { workerId: current.workerId, slot: current.slot, gen: current.gen };
      dependencies.postControl({ type: "facadeChildReady", ...identity }, current);
      phase = "trampoline";
      const result = glue.facade_child_trampoline(current.slot, current.gen, current.workerId);
      phase = "cleanup";
      if (result !== 0 && result !== 1) throw new Error(`facade trampoline 返回非法码 ${result}；只允许 0/1`);
      const intent = dependencies.environment.requireExitIntent();
      if (intent.workerId !== current.workerId || intent.slot !== current.slot || intent.gen !== current.gen) throw new Error("facade exit 意图身份与 bootstrap 不一致");
      // 必须先回到 JS；destroy 释放本实例 TLS/stack，是最后一次 WASM 调用。
      raw.__wbindgen_thread_destroy(0, 0, 0);
      const shutdown = new ChildShutdown({ postMessage: (message) => dependencies.postControl(message as WorkerExitMessage, current), close: () => { closePort(current.controlPort); closeScope(); } });
      shutdown.shutdown(current.workerId, current.slot, current.gen);
      finished = shutdown.isRustDone();
    } catch (error) {
      if (finished) {
        dependencies.reportError(new Error(`bootstrap 终结后加载任务仍失败：${snapshotFailure(error).message}`));
        return;
      }
      closeReceivedPort(value);
      fail(error);
    }
  }
  return { handleMessage, isFinished: () => finished };
}
