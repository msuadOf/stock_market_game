import { WasmMemoryStorage, WasmMemoryStorageError } from "./wasm-memory-storage.ts";
import type { WasmMemoryStorageRecovery, WasmMemoryReloadReady } from "./wasm-memory-storage.ts";
import { addWasmMemoryBytes, evaluateWasmMemoryBudget, WASM_MEMORY_MAXIMUM_BYTES, WASM_MEMORY_RECLAIM_BYTES } from "./wasm-memory-budget.ts";
import { parseWasmMemoryHandover } from "./wasm-memory-handover.ts";
import type { WasmMemoryHandover, WasmMemoryPendingIntent } from "./wasm-memory-handover.ts";
import { parseSetup } from "../save/schema/market.ts";
import { isFrozenJsonArchive, parseSaveSlot } from "../save/save-schema.ts";
import { validateDayEndArchive } from "../save/day-end-candidate.ts";
import { parsePlanU64 } from "../utils/plan-identity.ts";
import { WASM_PRODUCT_MEMORY_CAPACITY } from "./wasm-memory-capacity.ts";
import type { WasmEnforcedMemoryCapacity } from "./wasm-memory-capacity.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";
import { observeJsHeap } from "./wasm-memory-observation.ts";
import type { JsHeapObservation } from "./wasm-memory-observation.ts";
import { CHUNKED_RESTORE_EXPORTS, parseVerifiedRestoreCapability } from "./wasm-restore-package-capability.ts";
import type { WasmRestoreMode } from "./wasm-restore-package-capability.ts";
import { inspectValidatedArchiveLease } from "../save/validated-archive-lease.ts";
import type { ValidatedArchiveLease } from "../save/validated-archive-lease.ts";
import { inspectPinnedArchiveSource, type ChunkedSourceDescriptor, type PinnedArchiveSource } from "../save/chunked-handover-source.ts";
import { getBrowserChunkedHandoverSources } from "../save/browser-chunked-handover.ts";

type WasmDirectIntent = Exclude<WasmMemoryPendingIntent, null | ChunkedSourceDescriptor>;
export type WasmAdmissionIntent = WasmDirectIntent | Readonly<{ kind: "load-chunked"; lease: ValidatedArchiveLease }>;

/** released 只由真实终结器产生：全组终止、最后观测不可再 grow 的 Memory、清除全部强引用。 */
export type WasmFinalRelease = Readonly<{
  kind: "released";
  finalBytes: number;
  growthStopped: true;
  referencesDropped: true;
}> | Readonly<{ kind: "not-allocated" }>;

/** 工厂必须在首次 Worker/init 可能分配 Memory 之前同步登记一次，构造失败也须完成该回执。 */
export interface WasmMemoryAdmissionTicket {
  readonly intent: WasmAdmissionIntent;
  readonly selection: WasmEnforcedMemoryCapacity;
  readonly restoreCapability: VerifiedWasmRestoreCapability | null;
  readonly restoreMode: WasmRestoreMode | null;
  readonly productionAdmission?: false;
  registerRelease(release: Promise<WasmFinalRelease>): void;
}
const admittedIntents = new WeakMap<WasmMemoryAdmissionTicket, WasmAdmissionIntent>();
const chunkedTickets = new WeakMap<WasmMemoryAdmissionTicket, { lease: ValidatedArchiveLease; valid(): boolean; registered(): boolean }>();

/** 仅在 registerRelease 成功后读取此票唯一已校验且冻结的载入档。 */
export function requireAdmittedLoadArchive(ticket: WasmMemoryAdmissionTicket, supplied: unknown): StrictSaveEnvelope {
  const intent = admittedIntents.get(ticket);
  if (intent === undefined) throw new Error("Memory admission ticket 不是 owner 签发的票");
  if (intent.kind !== "load" || intent.archive !== supplied) throw new Error("初始档与 admission ticket 的冻结意图不一致");
  return intent.archive;
}
/** 仅签票本人且真实 release 已登记时访问唯一租约；开始 finish 即失效。 */
export function requireAdmittedChunkedLease(ticket: WasmMemoryAdmissionTicket, supplied: unknown): ValidatedArchiveLease {
  const record = chunkedTickets.get(ticket);
  if (record === undefined) throw new Error("Memory admission ticket 不是 owner 签发的分块票");
  if (!record.valid()) throw new Error("Memory admission ticket 已失效");
  if (!record.registered()) throw new Error("分块票尚未登记 Memory release");
  if (record.lease !== supplied) throw new Error("分块租约与 admission ticket 不一致");
  inspectValidatedArchiveLease(record.lease);
  return record.lease;
}
/** 来源是实际发布包导出和 glue 解析；装载器仍须在分配前核对真实 WASM 字节身份。 */
export interface VerifiedWasmRestoreCapability {
  readonly kind: "verified-package";
  readonly wasmSha256: string;
  readonly glueSha256: string;
  readonly sourceSha256: string;
  readonly buildManifestSha256: string;
  readonly exports: readonly string[];
}
export interface WasmRestoreCapabilitySource {
  forArchive(archive: StrictSaveEnvelope): VerifiedWasmRestoreCapability | null;
  forLease?(lease: ValidatedArchiveLease): VerifiedWasmRestoreCapability | null;
}
export type DocumentHostTransitionOutcome = { readonly kind: "released" } | { readonly kind: "failed"; readonly error: unknown };
export interface WasmDocumentMemoryOwnerOptions {
  maximumBytes: number;
  reclaimBytes: number;
  readCapacity(): WasmEnforcedMemoryCapacity | null;
  restoreCapabilities?: WasmRestoreCapabilitySource;
  createStorage(globalLiveUpperBoundBytes: number): WasmMemoryStorage;
  randomNonce?(): string;
  observeJsHeap?(): JsHeapObservation;
  inspectPinnedSource?(pin: PinnedArchiveSource, lease: ValidatedArchiveLease): ChunkedSourceDescriptor;
}
export class WasmDocumentMemoryError extends Error {
  readonly where = "wasm-document-memory-owner";
  readonly recoverable: boolean;
  readonly recoveryActions: readonly string[];
  readonly code: string;
  readonly context: Readonly<Record<string, unknown>>;
  constructor(code: string, message: string, context: Readonly<Record<string, unknown>> = {}, cause?: unknown) {
    super(message, { cause }); this.code = code; this.context = context;
    this.recoverable = code === "MEMORY_MEASUREMENT_NOT_READY" || code === "MEMORY_HANDOVER_CONFIRMATION_REQUIRED" || code === "MEMORY_RELOAD_RETRY_CONFIRMATION_REQUIRED";
    this.recoveryActions = this.recoverable ? ["确认日终存档或新局选择", "保留错误详情反馈"] : ["保留错误详情反馈", "关闭标签页后重新打开"];
  }
}

type ReleaseOutcome = { kind: "fulfilled"; value: WasmFinalRelease } | { kind: "rejected"; error: unknown };
type TicketState = {
  ticket: WasmMemoryAdmissionTicket;
  bound: number;
  release?: Promise<ReleaseOutcome>;
  finishing?: Promise<void>;
};
export type WasmMemoryAdmission<Intent extends WasmAdmissionIntent = WasmDirectIntent> = {
  kind: "admitted";
  ticket: WasmMemoryAdmissionTicket;
  intent: Intent;
} | ({ kind: "reload"; handover: WasmMemoryHandover } & Omit<WasmMemoryReloadReady, "kind">);

function integer(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw new WasmDocumentMemoryError("MEMORY_RELEASE_INVALID", `${field} 必须为非负 safe integer bytes`);
  return value;
}
function deepFreeze<Value>(value: Value): Value {
  if (isFrozenJsonArchive(value)) return value;
  if (value !== null && typeof value === "object") {
    for (const child of Object.values(value)) deepFreeze(child);
    Object.freeze(value);
  }
  return value;
}
function capacity(value: WasmEnforcedMemoryCapacity): WasmEnforcedMemoryCapacity {
  const copy = structuredClone(value);
  if (copy.kind !== "enforced-capacity" || integer(copy.initialPages, "initialPages") === 0
    || integer(copy.maximumPages, "maximumPages") < copy.initialPages || copy.maximumPages > 32768
    || integer(copy.liveUpperBoundBytes, "liveUpperBoundBytes") !== copy.maximumPages * 65536) {
    throw new WasmDocumentMemoryError("MEMORY_CAPACITY_INVALID", "Memory 上界必须来自实际 maximum 整页构造，不能使用当前长度或观测峰值");
  }
  return deepFreeze(copy);
}
export function validateOwnerRestoreCapability(value: VerifiedWasmRestoreCapability, mode: WasmRestoreMode = "string"): VerifiedWasmRestoreCapability {
  if (mode !== "string" && mode !== "chunked") throw new WasmDocumentMemoryError("WASM_RESTORE_TOPOLOGY_NOT_READY", "restore mode 必须为 string 或 chunked");
  const copy = structuredClone(value);
  if (copy.kind !== "verified-package" || !Array.isArray(copy.exports)
    || !copy.exports.includes("restore_json") || !copy.exports.includes("facade_slot_init_for_restore_json")
    || !copy.exports.includes("restore_prepared") || !copy.exports.includes("clear_prepared_restore")
    || (mode === "chunked" && CHUNKED_RESTORE_EXPORTS.some((name) => !copy.exports.includes(name)))) {
    throw new WasmDocumentMemoryError("WASM_RESTORE_TOPOLOGY_NOT_READY", `真实发布包尚未证明 ${mode} restore 所需导出，未消费存档或分配 Memory`);
  }
  for (const field of ["wasmSha256", "glueSha256", "sourceSha256", "buildManifestSha256"] as const) {
    if (typeof copy[field] !== "string" || !/^[a-f0-9]{64}$/.test(copy[field])) throw new WasmDocumentMemoryError("WASM_RESTORE_TOPOLOGY_NOT_READY", `真实发布包缺少 ${field} 身份绑定`);
  }
  if (mode === "chunked") {
    try { parseVerifiedRestoreCapability(copy, undefined, mode); }
    catch (cause) { throw new WasmDocumentMemoryError("WASM_RESTORE_TOPOLOGY_NOT_READY", "分块 restore package 身份或导出无效，未消费存档或分配 Memory", {}, cause); }
  }
  return deepFreeze(copy);
}
function freezeIntent(value: WasmDirectIntent): WasmDirectIntent {
  if (value.kind === "load") return { kind: "load", archive: validateDayEndArchive(isFrozenJsonArchive(value.archive) ? value.archive : parseSaveSlot(structuredClone(value.archive))) };
  if (value.kind !== "new") throw new Error("Memory 开局意图必须为 new 或 load");
  const setup = parseSetup(structuredClone(value.setup), "memory.intent.setup");
  if (setup.simulation_policy_id !== "a-share-simulation") throw new Error("Memory 新局必须使用 a-share-simulation");
  return { kind: "new", setup, seed: parsePlanU64(value.seed, "memory.intent.seed") };
}

/** 每 document 唯一所有者；React effect 只能借用，不能重置残留账或分叉身份。 */
export class WasmDocumentMemoryOwner {
  readonly hostTransitionRef: { current: Promise<DocumentHostTransitionOutcome> } = { current: Promise.resolve({ kind: "released" }) };
  private storage?: WasmMemoryStorage;
  private opened?: WasmMemoryStorageRecovery;
  private terminal?: { error: unknown };
  private selectedCapacity?: WasmEnforcedMemoryCapacity;
  private carryBase = 0;
  private sessionResident = 0;
  private reloadCount = 0;
  private active?: TicketState;
  private readonly tickets = new WeakMap<WasmMemoryAdmissionTicket, TicketState>();
  private handover?: WasmMemoryHandover;
  private reloadReady?: WasmMemoryAdmission & { kind: "reload" };
  private committedBoundary?: { tick: number; day: number };
  private freezeRetry?: WasmMemoryHandover & { kind: "frozen" };
  private jsObservation?: JsHeapObservation;

  private readonly options: WasmDocumentMemoryOwnerOptions;
  constructor(options: WasmDocumentMemoryOwnerOptions) { this.options = options; }
  get forkId(): string | undefined { return this.storage?.forkId; }
  private assertUsable(): void { if (this.terminal !== undefined) throw this.terminal.error; }
  private fail(error: unknown): never { if (this.terminal === undefined) this.terminal = { error }; throw this.terminal.error; }
  private ensureOpen(): void {
    this.assertUsable();
    if (this.opened !== undefined) return;
    const supplied = this.options.readCapacity();
    if (supplied === null) throw new WasmDocumentMemoryError("MEMORY_MEASUREMENT_NOT_READY", "缺少可信 Memory maximum 构造上界，尚未分配游戏 Memory；请保留当前选择并反馈。", { source: "enforced-capacity" });
    const chosen = capacity(supplied);
    const global = chosen.liveUpperBoundBytes;
    try {
      evaluateWasmMemoryBudget({ ...this.options, liveUpperBoundBytes: global, carryBase: 0, sessionResident: 0, liveBytes: 0 }, "admission");
      this.selectedCapacity = chosen;
      this.storage = this.options.createStorage(global);
      const recovery = this.storage.open();
      if (recovery.kind !== "restored" || recovery.carriedResidualBytes === null) throw new WasmDocumentMemoryError("MEMORY_MEASUREMENT_NOT_READY", "残留账重建未就绪，禁止分配 Memory");
      this.opened = recovery;
      this.carryBase = recovery.carriedResidualBytes;
      this.reloadCount = recovery.reloadCount;
      this.handover = recovery.handover;
    } catch (error) { this.fail(error); }
  }
  pendingHandover(): WasmMemoryHandover | undefined {
    this.ensureOpen();
    return this.handover === undefined ? undefined : structuredClone(this.handover);
  }
  private anchor(liveUpperBound: number) {
    return { carryBase: this.carryBase, sessionResident: this.sessionResident, liveUpperBound, reloadCount: this.reloadCount };
  }
  pendingReloadRetry(): (WasmMemoryHandover & { kind: "frozen" }) | undefined {
    this.assertUsable();
    return this.freezeRetry === undefined ? undefined : structuredClone(this.freezeRetry);
  }
  /** 同一 document 重挂时继续已发布的交接，不重新受理其他意图。 */
  pendingReloadReady(): (WasmMemoryAdmission & { kind: "reload" }) | undefined {
    this.assertUsable();
    return this.reloadReady === undefined ? undefined : structuredClone(this.reloadReady);
  }
  /** 仅明确确认原事件后调用；零参数禁止重传新 X、seed、nonce 或计数。 */
  retryReloadHandover(): WasmMemoryAdmission & { kind: "reload" } {
    this.assertUsable();
    if (this.freezeRetry === undefined) throw new Error("没有待明确重试的原冻结交接");
    try { return this.publishReload(this.storage!.retryReloadHandover(), this.freezeRetry); }
    catch (error) { return this.handleFreezeFailure(error); }
  }
  private handleFreezeFailure(error: unknown): never {
    if (error instanceof WasmMemoryStorageError && error.code === "MEMORY_HANDOVER_WRITE_FAILED"
      && error.context.channel === "session" && error.context.context === "freeze") throw error;
    return this.fail(error);
  }
  latestJsObservation(): JsHeapObservation | undefined {
    return this.jsObservation === undefined ? undefined : structuredClone(this.jsObservation);
  }
  private observeJsHeapAtAdmission(): void {
    if (this.options.observeJsHeap === undefined) return;
    try {
      this.jsObservation = this.options.observeJsHeap();
    } catch (cause) {
      if (cause instanceof WasmDocumentMemoryError) throw cause;
      this.fail(new WasmDocumentMemoryError("MEMORY_OBSERVATION_FAILED", "JS 面内存读取失败，未分配下一游戏 Memory；请保留详情反馈。", { face: "js" }, cause));
    }
  }
  private publishReload(ready: WasmMemoryReloadReady, handover: WasmMemoryHandover & { kind: "frozen" }): WasmMemoryAdmission & { kind: "reload" } {
    this.reloadCount = handover.reloadCount; this.handover = handover; this.freezeRetry = undefined;
    this.reloadReady = { ...ready, kind: "reload", handover };
    return structuredClone(this.reloadReady);
  }
  private budget(bound: number, phase: "admission" | "drop") {
    return evaluateWasmMemoryBudget({ ...this.options, liveUpperBoundBytes: bound, carryBase: this.carryBase, sessionResident: this.sessionResident, liveBytes: 0 }, phase);
  }
  private requireReady(status: string): void {
    if (status === "READY") return;
    const code = status === "NOT_READY" ? "MEMORY_MEASUREMENT_NOT_READY" : status === "EXHAUSTED" ? "MEMORY_RESIDUAL_EXHAUSTED" : "MEMORY_BUDGET_EXCEEDED";
    this.fail(new WasmDocumentMemoryError(code, `Memory 预算拒绝开局：${status}；请保留详情并关闭标签页后重新打开。`, { status }));
  }
  /** 预检只决定是否需要固定源；admission 仍重新计算预算并负责真正冻结。 */
  chunkedHandoverNeeded(): boolean {
    this.ensureOpen();
    if (this.handover !== undefined || this.sessionResident === 0) return false;
    return this.budget(this.selectedCapacity!.liveUpperBoundBytes, "drop").status === "RELOAD";
  }
  admit(value: WasmDirectIntent, confirmation: { confirmedHandover?: boolean } = {}): WasmMemoryAdmission {
    return this.admitInternal(value, confirmation);
  }
  /** 固定源凭证与租约绑定，达到重载阈值时冻结该源的描述。 */
  admitChunked(lease: ValidatedArchiveLease): { kind: "admitted"; ticket: WasmMemoryAdmissionTicket; intent: Extract<WasmAdmissionIntent, { kind: "load-chunked" }> };
  admitChunked(lease: ValidatedArchiveLease, options: { pinnedSource?: PinnedArchiveSource; confirmedHandover?: boolean }): WasmMemoryAdmission<Extract<WasmAdmissionIntent, { kind: "load-chunked" }>>;
  admitChunked(lease: ValidatedArchiveLease, options?: { pinnedSource?: PinnedArchiveSource; confirmedHandover?: boolean }): WasmMemoryAdmission<Extract<WasmAdmissionIntent, { kind: "load-chunked" }>> {
    inspectValidatedArchiveLease(lease);
    const descriptor = options?.pinnedSource === undefined ? undefined : (this.options.inspectPinnedSource ?? inspectPinnedArchiveSource)(options.pinnedSource, lease);
    return this.admitInternal({ kind: "load-chunked", lease }, { confirmedHandover: options?.confirmedHandover, descriptor });
  }
  private admitInternal(value: WasmDirectIntent, confirmation: { confirmedHandover?: boolean }): WasmMemoryAdmission;
  private admitInternal(value: Extract<WasmAdmissionIntent, { kind: "load-chunked" }>, confirmation: { confirmedHandover?: boolean; descriptor?: ChunkedSourceDescriptor }): WasmMemoryAdmission<Extract<WasmAdmissionIntent, { kind: "load-chunked" }>>;
  private admitInternal(value: WasmAdmissionIntent, confirmation: { confirmedHandover?: boolean; descriptor?: ChunkedSourceDescriptor }): WasmMemoryAdmission<WasmAdmissionIntent> {
    this.ensureOpen();
    if (this.active !== undefined) throw new WasmDocumentMemoryError("MEMORY_PREVIOUS_RELEASE_PENDING", "上一实例的真实 Memory 释放尚未完成，禁止创建下一实例");
    if (this.reloadReady !== undefined) throw new WasmDocumentMemoryError("MEMORY_RESIDUAL_RELOAD_HANDOVER", "原交接已保存，须重载后确认目标；当前文档不受理另一个开局意图。", { handover: structuredClone(this.reloadReady.handover) });
    if (this.freezeRetry !== undefined) throw new WasmDocumentMemoryError("MEMORY_RELOAD_RETRY_CONFIRMATION_REQUIRED", "上次交接写入失败，请明确确认重试原冻结目标；未受理本次其他选择。", { handover: structuredClone(this.freezeRetry) });
    if (this.handover !== undefined && value.kind === "load-chunked" && confirmation.confirmedHandover !== true) throw new WasmDocumentMemoryError("MEMORY_HANDOVER_CONFIRMATION_REQUIRED", "请先确认重载目标；未消费存档或分配 Memory。");
    if (this.handover !== undefined && confirmation.confirmedHandover !== true) throw new WasmDocumentMemoryError("MEMORY_HANDOVER_CONFIRMATION_REQUIRED", "请先确认继续冻结的新局或日终存档；确认前不会读取快速槽或分配游戏 Memory。", { handover: structuredClone(this.handover) });
    const intent: WasmAdmissionIntent = value.kind === "load-chunked" ? Object.freeze({ kind: "load-chunked", lease: value.lease }) : deepFreeze(freezeIntent(value));
    let verifiedRestore: VerifiedWasmRestoreCapability | null = null;
    if (intent.kind === "load") {
      const verified = this.options.restoreCapabilities === undefined ? null : this.options.restoreCapabilities.forArchive(intent.archive);
      if (verified === null) throw new WasmDocumentMemoryError("WASM_RESTORE_TOPOLOGY_NOT_READY", "当前发布包尚无已验证的 String restore 能力，未消费日终存档或分配 Memory。", { operation: "restore" });
      verifiedRestore = validateOwnerRestoreCapability(verified);
    }
    if (intent.kind === "load-chunked") {
      const source = this.options.restoreCapabilities;
      const verified = source?.forLease === undefined ? null : source.forLease(intent.lease);
      if (verified === null) throw new WasmDocumentMemoryError("WASM_RESTORE_TOPOLOGY_NOT_READY", "当前发布包尚无已验证的 chunked restore 能力，未消费日终存档或分配 Memory。", { operation: "restore" });
      verifiedRestore = validateOwnerRestoreCapability(verified, "chunked");
    }
    const chosen = this.selectedCapacity!;
    const bound = chosen.liveUpperBoundBytes;
    const admission = this.budget(bound, "admission");
    this.requireReady(admission.status);
    // 已有 drop 才检查 reload；无 drop 的零增量 F5 不是一次 drop。
    if (this.sessionResident > 0 && this.handover === undefined) {
      const drop = this.budget(this.selectedCapacity!.liveUpperBoundBytes, "drop");
      if (drop.status === "RELOAD") {
        if (intent.kind === "load-chunked" && confirmation.descriptor === undefined) throw new WasmDocumentMemoryError("WASM_CHUNKED_RELOAD_UNSUPPORTED", "分块存档不支持无固定源的 reload 交接；未消费存档或分配 Memory。");
        if (this.committedBoundary === undefined) throw new WasmDocumentMemoryError("MEMORY_RELOAD_BOUNDARY_UNAVAILABLE", "缺少最后已提交 tick/day，无法冻结可靠重载意图；未分配下一实例。", { phase: "freeze" });
        const handover = parseWasmMemoryHandover({ kind: "frozen", code: "MEMORY_RESIDUAL_RELOAD_HANDOVER", residualBytes: this.sessionResident,
          carriedResidualBytes: addWasmMemoryBytes(this.carryBase, this.sessionResident), thresholdEffective: drop.thresholdEffective,
          reloadCount: addWasmMemoryBytes(this.reloadCount, 1), pendingIntent: intent.kind === "load-chunked" ? confirmation.descriptor : intent,
          last_committed_tick: this.committedBoundary.tick, day: this.committedBoundary.day, handoverNonce: this.nonce() }, this.options.maximumBytes);
        if (handover.kind !== "frozen") throw new Error("内部冻结意图必须为 frozen");
        this.freezeRetry = handover;
        try {
          const ready = this.storage!.freezeReloadHandover({ ...this.anchor(0), reloadCount: this.reloadCount + 1 }, handover);
          return this.publishReload(ready, handover);
        } catch (error) { return this.handleFreezeFailure(error); }
      }
      this.requireReady(drop.status);
    }
    this.observeJsHeapAtAdmission();
    try { this.storage!.writeAdmissionSnapshot(this.anchor(bound)); }
    catch (error) {
      if (error instanceof WasmMemoryStorageError && error.context.channel === "session" && error.context.context === "consume") throw error;
      this.fail(error);
    }
    const state: TicketState = { ticket: { intent, selection: chosen, restoreCapability: verifiedRestore, restoreMode: intent.kind === "load-chunked" ? "chunked" : intent.kind === "load" ? "string" : null, ...(intent.kind === "load-chunked" ? { productionAdmission: false as const } : {}), registerRelease: (release) => {
      if (state.release !== undefined) throw new Error("每次 admission 只能登记一次 Memory release");
      if (this.active !== state) throw new Error("Memory admission ticket 已失效");
      state.release = Promise.resolve(release).then((result) => ({ kind: "fulfilled", value: result }), (error) => ({ kind: "rejected", error }));
    } }, bound };
    Object.freeze(state.ticket);
    this.active = state; this.tickets.set(state.ticket, state); admittedIntents.set(state.ticket, intent); this.handover = undefined;
    if (intent.kind === "load-chunked") chunkedTickets.set(state.ticket, { lease: intent.lease, valid: () => this.active === state && state.finishing === undefined, registered: () => state.release !== undefined });
    return { kind: "admitted", ticket: state.ticket, intent };
  }
  recordCommittedBoundary(ticket: WasmMemoryAdmissionTicket, boundary: { tick: number; day: number }): void {
    if (this.active?.ticket !== ticket) throw new Error("不能记录旧 Memory ticket 的提交边界");
    this.committedBoundary = { tick: integer(boundary.tick, "tick"), day: integer(boundary.day, "day") };
  }
  finish(ticket: WasmMemoryAdmissionTicket): Promise<void> {
    const state = this.tickets.get(ticket);
    if (state === undefined) return Promise.reject(new Error("不属于本 document owner 的 Memory ticket"));
    if (state.finishing !== undefined) return state.finishing;
    state.finishing = this.finishOnce(state);
    return state.finishing;
  }
  private async finishOnce(state: TicketState): Promise<void> {
    try {
      if (state.release === undefined) throw new WasmDocumentMemoryError("MEMORY_RELEASE_UNREGISTERED", "宿主工厂没有在可能分配前登记真实 Memory 终结回执，保留 live 上界并禁止下一局。");
      const outcome = await state.release;
      if (outcome.kind === "rejected") throw outcome.error;
      const result = outcome.value;
      let finalBytes: number;
      if (result.kind === "not-allocated") finalBytes = 0;
      else {
        if (result.kind !== "released" || result.growthStopped !== true || result.referencesDropped !== true) throw new WasmDocumentMemoryError("MEMORY_RELEASE_INVALID", "Memory 终结回执未证明停止增长与清除强引用");
        finalBytes = integer(result.finalBytes, "finalBytes");
      }
      this.sessionResident = addWasmMemoryBytes(this.sessionResident, finalBytes);
      const total = addWasmMemoryBytes(this.carryBase, this.sessionResident);
      if (total > this.options.maximumBytes) {
        // codec 不接受超 maximum 的账；最坏 maximum 锚保证 F5 后继续显式耗尽。
        this.storage!.writeSnapshot({ ...this.anchor(0), sessionResident: this.options.maximumBytes - this.carryBase });
        throw new WasmDocumentMemoryError("MEMORY_BUDGET_EXCEEDED", "实际终值已突破总 Memory 预算，已写入最坏上界锚并禁止后继实例。", { total, maximumBytes: this.options.maximumBytes });
      }
      this.storage!.writeSnapshot(this.anchor(0));
      this.active = undefined;
      if (finalBytes > state.bound) throw new WasmDocumentMemoryError("MEMORY_CAPACITY_BOUND_EXCEEDED", "实际 Memory 终值超过本次强制构造上界，已保守记账并禁止后继实例。", { finalBytes, liveUpperBoundBytes: state.bound });
      if (result.kind === "released") {
        const decision = this.budget(this.selectedCapacity!.liveUpperBoundBytes, "drop");
        if (decision.status !== "RELOAD") this.requireReady(decision.status);
      }
    } catch (error) { this.fail(error); }
  }
  private nonce(): string {
    if (this.options.randomNonce !== undefined) return this.options.randomNonce();
    const bytes = new Uint8Array(16); globalThis.crypto.getRandomValues(bytes);
    return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
  }
}

const owners = new WeakMap<object, WasmDocumentMemoryOwner>();
const restoreSources = new WeakMap<object, WasmRestoreCapabilitySource>();
/** 发布包 producer 安装实际导出/身份能力源；该接口不把容量或 observed sample 当恢复支持。 */
export function configureDocumentWasmRestoreCapabilities(documentKey: object, source: WasmRestoreCapabilitySource): void {
  if (restoreSources.has(documentKey)) throw new Error("每 document 只能安装一次真实 WASM restore capability source");
  restoreSources.set(documentKey, source);
}
export function getDocumentWasmMemoryOwner(documentKey: object, create?: () => WasmDocumentMemoryOwner): WasmDocumentMemoryOwner {
  const existing = owners.get(documentKey);
  if (existing !== undefined) return existing;
  const owner = create === undefined ? new WasmDocumentMemoryOwner({ maximumBytes: WASM_MEMORY_MAXIMUM_BYTES, reclaimBytes: WASM_MEMORY_RECLAIM_BYTES,
    readCapacity: () => WASM_PRODUCT_MEMORY_CAPACITY,
    observeJsHeap: () => observeJsHeap(globalThis.performance),
    inspectPinnedSource: (pin, lease) => getBrowserChunkedHandoverSources(documentKey).inspect(pin, lease),
    restoreCapabilities: { forArchive: (archive) => {
      const source = restoreSources.get(documentKey); return source === undefined ? null : source.forArchive(archive);
    }, forLease: (lease) => {
      const source = restoreSources.get(documentKey);
      return source?.forLease === undefined ? null : source.forLease(lease);
    } },
    createStorage: (liveUpperBoundBytes) => new WasmMemoryStorage({ session: globalThis.sessionStorage, ledger: globalThis.localStorage,
      maximumBytes: WASM_MEMORY_MAXIMUM_BYTES, reclaimBytes: WASM_MEMORY_RECLAIM_BYTES, liveUpperBoundBytes }),
  }) : create();
  owners.set(documentKey, owner); return owner;
}
