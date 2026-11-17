import { CHECKPOINT_EXIT_BUDGET_MS, shutdownClock } from "./wasm-shutdown-budget.ts";
// 权威会话的独立归档连接；连接只发送小型控制信号，不传存档正文。
import { browserWorkerEntry, cancelTimeout, scheduleTimeout, spawnModuleWorker, type TimeoutHandle } from "../../../../packages/libos/browser/browser-transport.ts";
import type { FacadeSubstrate } from "./engine-supervisor.ts";

const workerAssets = import.meta.env === undefined ? undefined : import.meta.glob<string>(
  ["./wasm-checkpoint-worker.ts"], { eager: true, import: "default", query: "?worker&url" },
);

export interface CheckpointBindings {
  market_archive_attach(handle: number, generation: bigint, writerEpoch: bigint): void;
  market_archive_detach(handle: number): void;
  market_archive_latest(handle: number): string | null | undefined;
  market_archive_receipt(handle: number, generation: bigint, writerEpoch: bigint, sequence: bigint, failure?: string): "durable" | "stale" | "failed";
  market_archive_failed(handle: number, generation: bigint, writerEpoch: bigint, message: string): void;
  market_archive_finish(handle: number, abandon: boolean): void;
  market_archive_tombstone(): number;
}

type ReportArchiveError = (error: Error, scope: "item" | "connection") => void;

type Pending = { seq: bigint; resolve(): void; reject(error: Error): void };
const DEADLINE_MS = CHECKPOINT_EXIT_BUDGET_MS;

export class CheckpointCloseError extends Error {
  readonly quiescence: "confirmed" | "unknown";
  constructor(quiescence: "confirmed" | "unknown", cause: unknown) {
    super(`WASM_ARCHIVE_${quiescence === "confirmed" ? "NONDURABLE" : "QUIESCENCE_UNKNOWN"}: ${cause instanceof Error ? cause.message : String(cause)}`, { cause });
    this.name = "CheckpointCloseError";
    this.quiescence = quiescence;
  }
}

class CheckpointConnection {
  private readonly ready: Promise<void>;
  private readonly exited: Promise<void>;
  private readonly waiting = new Set<Pending>();
  private phase: "starting" | "ready" | "closing" | "closed" = "starting";
  private failure: Error | null = null;
  private stored = -1n;
  private completed = -1n;
  private writeFailure: Error | null = null;
  private clean = false;
  private closeRequested = false;
  private readonly detach: () => void;
  private readonly finish: (abandon: boolean) => void;
  private readonly report: ReportArchiveError;
  private readonly forceUnknownExit: () => void;

  constructor(
    worker: Worker, handle: number, bindings: CheckpointBindings, generation: number, writerEpoch: bigint,
    substrate: FacadeSubstrate, report: ReportArchiveError,
  ) {
    this.report = report;
    let ready!: () => void, failReady!: (error: Error) => void, exit!: () => void;
    this.ready = new Promise<void>((resolve, reject) => { ready = resolve; failReady = reject; });
    this.exited = new Promise<void>((resolve) => { exit = resolve; });
    const dead = (error: Error): void => {
      if (this.phase === "closed") return;
      this.failure = error;
      const wasReady = this.phase === "ready";
      this.phase = "closed";
      failReady(error);
      for (const waiter of this.waiting) waiter.reject(error);
      this.waiting.clear();
      if (this.clean) exit();
      if (wasReady) {
        bindings.market_archive_failed(handle, BigInt(generation), writerEpoch, error.message);
        this.report(error, "connection");
      }
    };
    this.forceUnknownExit = () => {
      if (import.meta.env === undefined || !import.meta.env.DEV || import.meta.env.MODE !== "e2e" || !substrate.diagnostics) throw new Error("强退注入仅允许 diagnostics E2E 验证环境");
      worker.terminate();
      dead(new Error("验证注入：归档 Worker 物理强退，Rust 资源退出未知"));
    };
    worker.addEventListener("message", (event: MessageEvent<unknown>) => {
      const message = event.data;
      if (this.phase === "closed") {
        // 错误报告不是 Rust 退出证明；允许迟到的清理收据补齐 quiescence。
        if (message !== null && typeof message === "object") {
          const final = message as Readonly<Record<string, unknown>>;
          if ((final.type === "failed" && final.clean === true) || (this.closeRequested && final.type === "closed")) {
            this.clean = true;
            exit();
          }
        }
        return;
      }
      if (message === null || typeof message !== "object") { dead(new Error("归档 Worker 响应无效")); return; }
      const value = message as Readonly<Record<string, unknown>>;
      if (value.type === "ready" && this.phase === "starting") {
        this.phase = "ready";
        ready();
      } else if ((value.type === "stored" || (value.type === "writeFailed" && typeof value.message === "string"))
        && this.phase !== "starting" && typeof value.seq === "string" && /^(0|[1-9][0-9]*)$/.test(value.seq)) {
        const after = BigInt(value.seq);
        if (after > 0xffffffffffffffffn) { dead(new Error("归档 Worker 的完成序号溢出")); return; }
        if (after <= this.completed) return; // 已完成的重复传输不释放第二份信用。
        if (this.writeFailure !== null && value.type === "stored") {
          dead(new Error("归档仍处于 blocked，较晚成功包不能越过失败候选")); return;
        }
        let disposition: "durable" | "stale" | "failed";
        try { disposition = bindings.market_archive_receipt(handle, BigInt(generation), writerEpoch, after, value.type === "writeFailed" ? String(value.message) : undefined); }
        catch (error) { dead(new Error("归档收据未被权威会话接受", { cause: error })); return; }
        if (disposition === "stale") return;
        if ((value.type === "stored" && disposition !== "durable") || (value.type === "writeFailed" && disposition !== "failed")) {
          dead(new Error("Rust 权威未确认当前归档耐久，存储收据与权威状态不一致")); return;
        }
        this.completed = after;
        const writeFailure = value.type === "writeFailed" ? new Error(`日终归档 ${after} 写入失败：${value.message}`) : null;
        if (writeFailure !== null && this.writeFailure === null) this.writeFailure = writeFailure;
        if (writeFailure === null) this.stored = after;
        for (const waiter of this.waiting) if (waiter.seq <= after) {
          this.waiting.delete(waiter);
          if (writeFailure === null) waiter.resolve();
          else waiter.reject(writeFailure);
        }
        if (writeFailure !== null) this.report(writeFailure, "item");
      } else if (value.type === "closed" && this.phase === "closing") {
        this.phase = "closed";
        this.clean = true;
        exit();
      } else if (value.type === "failed" && typeof value.message === "string") {
        this.clean = value.clean === true;
        dead(new Error(value.message));
      } else {
        dead(new Error("归档 Worker 响应类型或世代无效"));
      }
    });
    worker.addEventListener("error", (event) => dead(new Error(event.message || "归档 Worker 执行失败")));
    worker.addEventListener("messageerror", () => dead(new Error("归档 Worker 消息解码失败")));
    this.detach = () => bindings.market_archive_detach(handle);
    this.finish = abandon => bindings.market_archive_finish(handle, abandon);
    bindings.market_archive_attach(handle, BigInt(generation), writerEpoch);
    const address = bindings.market_archive_tombstone();
    worker.postMessage({
      type: "bindCheckpoint", generation: String(generation), writerEpoch: String(writerEpoch),
      lease: crypto.randomUUID(), address, module: substrate.module, memory: substrate.memory, layout: substrate.layout, diagnostics: substrate.diagnostics,
    });
  }

  forceUnknownExitForVerification(): void { this.forceUnknownExit(); }

  waitReady(): Promise<void> { return bounded(this.ready, "归档 Worker 未就绪"); }

  async waitStored(wanted: string | null | undefined): Promise<void> {
    if (this.failure !== null) throw this.failure;
    if (this.writeFailure !== null) throw this.writeFailure;
    if (wanted === null || wanted === undefined) return;
    const seq = BigInt(wanted);
    if (seq <= this.stored) return;
    let pending!: Pending;
    const result = new Promise<void>((resolve, reject) => { pending = { seq, resolve, reject }; this.waiting.add(pending); });
    try { await bounded(result, "归档写入未在期限内完成"); }
    finally { this.waiting.delete(pending); }
  }

  async close(abandon: boolean, deadline: number): Promise<void> {
    this.closeRequested = true;
    try {
      this.detach();
      if (this.phase !== "closed") this.phase = "closing";
      if (!this.clean) await bounded(this.exited, "归档 Worker 尚未证明释放共享 Memory 中的资源，需重开运行环境", Math.max(0, Math.min(CHECKPOINT_EXIT_BUDGET_MS, deadline - shutdownClock() - 1_000)));
    } catch (error) { throw new CheckpointCloseError(this.clean ? "confirmed" : "unknown", error); }
    let primary: unknown = abandon ? undefined : this.failure ?? this.writeFailure ?? undefined;
    try { this.finish(abandon); }
    catch (error) { primary = primary === undefined ? error : new AggregateError([primary, error], "归档主错与耐久围栏失败", { cause: primary }); }
    if (primary !== undefined) {
      // 确认 consumer 已退出，关停可销毁未耐久候选，但必须保留失败事实。
      // restore 的显式 abandon 仅在 finish 成功后才允许安装下一 writer。
      if (!abandon) {
        try { this.finish(true); }
        catch (cleanup) { primary = new AggregateError([primary, cleanup], "归档未耐久且撤销失败", { cause: primary }); }
      }
      throw new CheckpointCloseError("confirmed", primary);
    }
  }
}

function bounded<T>(operation: Promise<T>, message: string, budget = DEADLINE_MS): Promise<T> {
  let timer!: TimeoutHandle;
  const deadline = new Promise<never>((_resolve, reject) => { timer = scheduleTimeout(() => reject(new Error(message)), budget); });
  return Promise.race([operation, deadline]).finally(() => cancelTimeout(timer));
}

export class WasmCheckpointClient {
  private current: CheckpointConnection | null = null;
  private writerEpoch = 0n;

  attach(
    handle: number, wasm: unknown, generation: number, substrate: FacadeSubstrate,
    report: ReportArchiveError,
  ): Promise<void> | undefined {
    // 直接执行的 Node 桩没有 Worker；Vite 浏览器构建的真实路径必须具备完整 ABI。
    if (import.meta.env === undefined && typeof Worker === "undefined") return;
    if (this.current !== null) throw new Error("旧归档副实例尚未退出");
    const bindings = wasm as Partial<CheckpointBindings>;
    for (const member of ["market_archive_attach", "market_archive_detach", "market_archive_latest", "market_archive_receipt", "market_archive_failed", "market_archive_finish", "market_archive_tombstone"] as const) {
      if (typeof bindings[member] !== "function") throw new Error(`WASM 缺少归档 ABI ${member}`);
    }
    const source = "./wasm-checkpoint-worker.ts";
    const worker = spawnModuleWorker(browserWorkerEntry(source, import.meta.url, workerAssets), "stock-game:checkpoint");
    if (this.writerEpoch === 0xffffffffffffffffn) throw new Error("归档 writer epoch 已耗尽");
    this.writerEpoch += 1n;
    this.current = new CheckpointConnection(worker, handle, bindings as CheckpointBindings, generation, this.writerEpoch, substrate, report);
    return this.current.waitReady();
  }

  forceUnknownExitForVerification(): void {
    if (this.current === null) throw new Error("验证注入缺少活动归档连接");
    this.current.forceUnknownExitForVerification();
  }

  async waitIdle(handle: number, wasm: unknown): Promise<void> {
    if (this.current === null) return;
    await this.current.waitStored((wasm as CheckpointBindings).market_archive_latest(handle));
  }

  close(abandon = false, deadline = shutdownClock() + CHECKPOINT_EXIT_BUDGET_MS + 1_000): Promise<void> | undefined {
    const current = this.current;
    if (current === null) return;
    return current.close(abandon, deadline).then(() => { if (this.current === current) this.current = null; }, error => {
      if (error instanceof CheckpointCloseError && error.quiescence === "confirmed" && this.current === current) this.current = null;
      throw error;
    });
  }
}
