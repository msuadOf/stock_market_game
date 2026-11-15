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
  market_archive_receipt(handle: number, generation: bigint, sequence: bigint): void;
  market_archive_tombstone(): number;
}

type ReportArchiveError = (error: Error, scope: "item" | "connection") => void;

type Pending = { seq: bigint; resolve(): void; reject(error: Error): void };
const DEADLINE_MS = 10_000;

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
  private readonly detach: () => void;
  private readonly report: ReportArchiveError;

  constructor(
    worker: Worker, handle: number, bindings: CheckpointBindings, generation: number,
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
      exit();
      if (wasReady) this.report(error, "connection");
    };
    worker.addEventListener("message", (event: MessageEvent<unknown>) => {
      if (this.phase === "closed") return;
      const message = event.data;
      if (message === null || typeof message !== "object") { dead(new Error("归档 Worker 响应无效")); return; }
      const value = message as Readonly<Record<string, unknown>>;
      if (value.type === "ready" && this.phase === "starting") {
        this.phase = "ready";
        ready();
      } else if ((value.type === "stored" || (value.type === "writeFailed" && typeof value.message === "string"))
        && this.phase !== "starting" && typeof value.seq === "string" && /^(0|[1-9][0-9]*)$/.test(value.seq)) {
        const after = BigInt(value.seq);
        if (after <= this.completed || after > 0xffffffffffffffffn) {
          dead(new Error("归档 Worker 的完成序号无效或未递增")); return;
        }
        try { bindings.market_archive_receipt(handle, BigInt(generation), after); }
        catch (error) { dead(new Error("归档收据未被权威会话接受", { cause: error })); return; }
        this.completed = after;
        const writeFailure = value.type === "writeFailed" ? new Error(`日终归档 ${after} 写入失败：${value.message}`) : null;
        this.writeFailure = writeFailure;
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
    bindings.market_archive_attach(handle, BigInt(generation), BigInt(generation));
    const address = bindings.market_archive_tombstone();
    worker.postMessage({
      type: "bindCheckpoint", generation: String(generation), writerEpoch: String(generation),
      lease: crypto.randomUUID(), address, module: substrate.module, memory: substrate.memory, layout: substrate.layout, diagnostics: substrate.diagnostics,
    });
  }

  waitReady(): Promise<void> { return bounded(this.ready, "归档 Worker 未就绪"); }

  async waitStored(wanted: string | null | undefined): Promise<void> {
    if (this.failure !== null) throw this.failure;
    if (wanted === null || wanted === undefined) return;
    const seq = BigInt(wanted);
    if (seq <= this.stored) return;
    if (seq <= this.completed && this.writeFailure !== null) throw this.writeFailure;
    let pending!: Pending;
    const result = new Promise<void>((resolve, reject) => { pending = { seq, resolve, reject }; this.waiting.add(pending); });
    try { await bounded(result, "归档写入未在期限内完成"); }
    finally { this.waiting.delete(pending); }
  }

  async close(): Promise<void> {
    this.detach();
    if (this.phase !== "closed") {
      this.phase = "closing";
      await bounded(this.exited, "归档 Worker 未完成排空和关闭");
    }
    if (!this.clean && this.failure !== null) throw this.failure;
  }
}

function bounded<T>(operation: Promise<T>, message: string): Promise<T> {
  let timer!: TimeoutHandle;
  const deadline = new Promise<never>((_resolve, reject) => { timer = scheduleTimeout(() => reject(new Error(message)), DEADLINE_MS); });
  return Promise.race([operation, deadline]).finally(() => cancelTimeout(timer));
}

export class WasmCheckpointClient {
  private current: CheckpointConnection | null = null;

  attach(
    handle: number, wasm: unknown, generation: number, substrate: FacadeSubstrate,
    report: ReportArchiveError,
  ): Promise<void> | undefined {
    // 直接执行的 Node 桩没有 Worker；Vite 浏览器构建的真实路径必须具备完整 ABI。
    if (import.meta.env === undefined && typeof Worker === "undefined") return;
    if (this.current !== null) throw new Error("旧归档副实例尚未退出");
    const bindings = wasm as Partial<CheckpointBindings>;
    for (const member of ["market_archive_attach", "market_archive_detach", "market_archive_latest", "market_archive_receipt", "market_archive_tombstone"] as const) {
      if (typeof bindings[member] !== "function") throw new Error(`WASM 缺少归档 ABI ${member}`);
    }
    const source = "./wasm-checkpoint-worker.ts";
    const worker = spawnModuleWorker(browserWorkerEntry(source, import.meta.url, workerAssets), "stock-game:checkpoint");
    this.current = new CheckpointConnection(worker, handle, bindings as CheckpointBindings, generation, substrate, report);
    return this.current.waitReady();
  }

  async waitIdle(handle: number, wasm: unknown): Promise<void> {
    if (this.current === null) return;
    await this.current.waitStored((wasm as CheckpointBindings).market_archive_latest(handle));
  }

  close(): Promise<void> | undefined {
    const current = this.current;
    if (current === null) return;
    return current.close().then(() => { if (this.current === current) this.current = null; });
  }
}
