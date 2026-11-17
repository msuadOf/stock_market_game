import { parseRustSessionShutdown, type WasmSessionShutdown } from "./wasm-shutdown-report.ts";
import { describeWasmFailure } from "./wasm-failure.ts";
import { restoreWasmSession } from "./wasm-restore-transaction.ts";

export interface WasmSessionBindings {
  readonly create_session: (setup: unknown, seed: bigint) => number;
  readonly restore_json: (slot: string) => number;
  readonly restore_prepared?: () => number;
  readonly prepare_session_replacement?: (previous: number, candidate: number, generation: bigint) => void;
  readonly fence_session_replacement?: (previous: number, candidate: number, generation: bigint) => void;
  readonly commit_session_replacement?: (previous: number, candidate: number, generation: bigint) => bigint;
  readonly snapshot: (handle: number) => unknown;
  readonly shutdown_session?: (handle: number) => unknown;
  readonly drop_session: (handle: number) => void;
  readonly prepare_public_baseline?: (handle: number) => void;
}

/** prepare 阶段只拥有未激活 candidate；不能同时安装第二个归档 CONSUMER。 */
export interface PreparedWasmRestore {
  readonly previous: number;
  readonly generation: number;
  readonly candidate: number;
  readonly snapshot: unknown;
  consumed: boolean;
  fenced?: boolean;
}

export interface WasmRestoreLoopHooks {
  readonly wasRunning: boolean;
  readonly stop: () => void;
  readonly restart: () => void;
}

/** 仅在 shutdown report 解码失败时生成诊断纯壳；Error 自定义字段不能交给原生 Error clone。 */
function shutdownErrorSnapshot(value: unknown, seen = new WeakSet<object>()): unknown {
  if (value === null || typeof value !== "object") {
    return typeof value === "function" || typeof value === "symbol" ? { unavailable: "错误值不可克隆", description: describeWasmFailure(value) } : value;
  }
  if (seen.has(value)) return { unavailable: "错误详情包含循环引用" };
  seen.add(value);
  try {
    // 诊断不能延长真实 WASM Memory 的 ownership，也不能把共享内存发送给主线程。
    if (value instanceof WebAssembly.Memory || value instanceof SharedArrayBuffer || value instanceof ArrayBuffer || ArrayBuffer.isView(value)) return { unavailable: "内存资源不进入 shutdown 错误详情" };
    const descriptors = Object.getOwnPropertyDescriptors(value);
    const fields = Object.fromEntries(Object.entries(descriptors).filter(([key]) => key !== "stack").map(([key, descriptor]) => [key,
      "value" in descriptor ? shutdownErrorSnapshot(descriptor.value, seen) : "accessor 字段未读取，原值不可克隆",
    ]));
    if (value instanceof Error && !Object.hasOwn(fields, "name")) {
      try { fields.name = shutdownErrorSnapshot(value.name, seen); }
      catch (nameError) { fields.name = { unavailable: "读取 Error.name 失败", description: describeWasmFailure(nameError) }; }
    }
    // 按 descriptor 重建数组，不重新触发 index getter。
    return Array.isArray(value) ? Array.from({ length: descriptors.length!.value as number }, (_, index) => fields[String(index)]) : fields;
  } catch (snapshotError) {
    return { unavailable: "读取 shutdown 错误详情失败", description: describeWasmFailure(value), snapshotError: describeWasmFailure(snapshotError) };
  } finally { seen.delete(value); }
}

export const WASM_CREATE_ONCE_ERROR = "Worker 会话只能初始化一次；失败后必须重建 Worker";

/** bindings 的异步初始化归 Worker；当前 handle 与 generation 只归此槽。 */
export class WasmSessionSlot<TBindings extends WasmSessionBindings> {
  private handle: number | null = null;
  private generation = 0;
  private createAttempted = false;
  private initialRestoreAttempted = false;
  private readonly bindings: () => TBindings | null;

  constructor(bindings: () => TBindings | null) {
    this.bindings = bindings;
  }

  readGeneration(): number {
    return this.generation;
  }

  requireGeneration(requested: unknown): number {
    if (!Number.isSafeInteger(requested) || requested !== this.generation) {
      throw new Error("Worker 请求属于已过期会话");
    }
    return this.generation;
  }

  requireHandle(): [number, TBindings] {
    const wasm = this.bindings();
    if (this.handle === null || wasm === null) throw new Error("Worker 会话尚未就绪");
    return [this.handle, wasm];
  }

  create(setup: unknown, seed: unknown, onFfiStart?: () => void): void {
    const wasm = this.bindings();
    if (wasm === null) throw new Error("wasm 未初始化");
    if (typeof seed !== "bigint") throw new Error("seed 必须是 bigint");
    // create_session 失败时 Rust 侧可能已经构造或登记过资源；本槽无法证明可回收，
    // 因此首次尝试前即锁存，重复或失败后的 create 都只能由新 Worker 承担。
    if (this.createAttempted || this.initialRestoreAttempted || this.handle !== null) {
      throw new Error(WASM_CREATE_ONCE_ERROR);
    }
    this.createAttempted = true;
    onFfiStart?.();
    this.handle = wasm.create_session(setup, seed);
    this.generation += 1;
  }

  restoreInitial(): void {
    if (this.initialRestoreAttempted || this.createAttempted || this.handle !== null || this.generation !== 0) throw new Error("restoreInitial 仅允许首次空槽调用一次，不能替换已有会话");
    const wasm = this.bindings();
    if (wasm === null) throw new Error("wasm 未初始化");
    if (typeof wasm.restore_prepared !== "function") throw new Error("WASM 缺少初始档消费接口");
    this.initialRestoreAttempted = true;
    const handle = wasm.restore_prepared();
    if (!Number.isInteger(handle) || handle < 1 || handle > 0xffff_ffff) throw new Error("restore_prepared 返回非法 SessionHandle");
    this.handle = handle;
    this.generation = 1;
  }

  prepareRestore(parsedSlot: unknown): PreparedWasmRestore {
    const [previous, wasm] = this.requireHandle();
    if (typeof wasm.commit_session_replacement !== "function" || typeof wasm.prepare_session_replacement !== "function" || typeof wasm.fence_session_replacement !== "function") throw new Error("WASM 缺少共享 authority 替换接口，请重建 bindings");
    const candidate = wasm.restore_json(JSON.stringify(parsedSlot));
    try {
      const snapshot = wasm.snapshot(candidate);
      wasm.prepare_session_replacement(previous, candidate, BigInt(this.generation));
      return { previous, generation: this.generation, candidate, snapshot, consumed: false };
    } catch (primary) {
      try { wasm.drop_session(candidate); }
      catch (cleanup) { throw new AggregateError([primary, cleanup], `恢复准备与候选清理均失败：${describeWasmFailure(primary)}；${describeWasmFailure(cleanup)}`, { cause: primary }); }
      throw primary;
    }
  }

  fencePreparedRestore(prepared: PreparedWasmRestore): void {
    const [previous, wasm] = this.requireHandle();
    if (prepared.consumed || prepared.previous !== previous || prepared.generation !== this.generation) throw new Error("恢复候选已过期");
    if (prepared.fenced) throw new Error("旧会话已经封口");
    if (typeof wasm.fence_session_replacement !== "function") throw new Error("WASM 缺少真实 ingress 退役 fence");
    wasm.fence_session_replacement(previous, prepared.candidate, BigInt(this.generation));
    prepared.fenced = true;
  }

  discardPreparedRestore(prepared: PreparedWasmRestore): void {
    if (prepared.consumed) return;
    prepared.consumed = true;
    const [, wasm] = this.requireHandle();
    wasm.drop_session(prepared.candidate);
  }

  restore(parsedSlot: unknown, loopHooks: WasmRestoreLoopHooks, prepared?: PreparedWasmRestore): unknown {
    const [previous, wasm] = this.requireHandle();
    const commit = wasm.commit_session_replacement;
    if (typeof commit !== "function") throw new Error("WASM 缺少共享 authority 替换接口，请重建 bindings");
    if (prepared !== undefined && (prepared.consumed || prepared.previous !== previous || prepared.generation !== this.generation)) throw new Error("恢复候选属于已过期或已消费计划");
    if (prepared !== undefined) prepared.consumed = true;
    const restoredSnapshot = restoreWasmSession({
      currentHandle: () => this.handle,
      replaceHandle: (handle) => {
        if (prepared === undefined) {
          if (typeof wasm.prepare_session_replacement !== "function") throw new Error("WASM 缺少共享 authority prepare 接口");
          wasm.prepare_session_replacement(previous, handle, BigInt(this.generation));
        }
        const next = commit(previous, handle, BigInt(this.generation));
        // ABI 成功即表示旧入口已失效；本地立即接纳提交结果，不伪装回滚。
        this.handle = handle;
        this.generation = Number(next);
        if (!Number.isSafeInteger(this.generation) || this.generation <= 0) throw new Error("Rust authority 返回的 generation 超过 Browser 安全整数范围");
      },
      restore: () => prepared === undefined ? wasm.restore_json(JSON.stringify(parsedSlot)) : prepared.candidate,
      snapshot: prepared === undefined ? wasm.snapshot : () => prepared.snapshot,
      drop: wasm.drop_session,
      afterCommit: () => {
        if (this.handle === null) throw new Error("WASM restore 没有安装新会话句柄");
        this.preparePublicBaseline(this.handle, wasm);
      },
      ...loopHooks,
    });
    // authority 一旦替换就已经推进 generation；旧句柄清理失败也不得继续接旧请求。
    if (this.handle === null) throw new Error("WASM restore 没有安装新会话句柄");
    return restoredSnapshot;
  }

  prepareBaseline(): void {
    const [handle, wasm] = this.requireHandle();
    this.preparePublicBaseline(handle, wasm);
  }

  shutdown(): WasmSessionShutdown {
    const [handle, wasm] = this.requireHandle();
    const shutdown = wasm.shutdown_session;
    if (typeof shutdown !== "function") throw new Error("当前 WASM bindings 缺少 shutdown_session，请重建 bindings");
    // shutdown_session 消费 Rust registry ownership；调用前撤销本地 handle，失败也不得重用。
    this.handle = null;
    let raw: unknown;
    try { raw = shutdown(handle); }
    catch (ffiError) {
      try { return parseRustSessionShutdown(ffiError, "failure", handle); }
      catch (decoderError) {
        throw Object.assign(new Error("shutdown report 无法解码；保留原 FFI 故障与 decoder 详情", {
          cause: { ffiError: shutdownErrorSnapshot(ffiError), decoderError: shutdownErrorSnapshot(decoderError) },
        }), { code: "WASM_SHUTDOWN_REPORT_INVALID", where: "wasm-session-slot.shutdown" });
      }
    }
    return parseRustSessionShutdown(raw, "success", handle);
  }

  drop(): void {
    const wasm = this.bindings();
    if (this.handle !== null && wasm !== null) wasm.drop_session(this.handle);
    this.handle = null;
  }

  private preparePublicBaseline(handle: number, wasm: TBindings): void {
    const prepare = wasm.prepare_public_baseline;
    if (prepare === undefined) throw new Error("当前 WASM bindings 缺少公开基线准备接口，请重建 bindings");
    prepare(handle);
  }
}
