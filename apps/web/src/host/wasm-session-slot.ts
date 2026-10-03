import { restoreWasmSession } from "./wasm-restore-transaction.ts";

export interface WasmSessionBindings {
  readonly create_session: (setup: unknown, seed: bigint) => number;
  readonly restore_json: (slot: string) => number;
  readonly snapshot: (handle: number) => unknown;
  readonly drop_session: (handle: number) => void;
  readonly prepare_public_baseline?: (handle: number) => void;
}

export interface WasmRestoreLoopHooks {
  readonly wasRunning: boolean;
  readonly stop: () => void;
  readonly restart: () => void;
}

/** bindings 的异步初始化归 Worker；当前 handle 与 generation 只归此槽。 */
export class WasmSessionSlot<TBindings extends WasmSessionBindings> {
  private handle: number | null = null;
  private generation = 0;
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

  create(setup: unknown, seed: unknown): void {
    const wasm = this.bindings();
    if (wasm === null) throw new Error("wasm 未初始化");
    if (typeof seed !== "bigint") throw new Error("seed 必须是 bigint");
    // 保留重复 create 直接覆盖的现状，资源缺陷须另批修复。
    this.handle = wasm.create_session(setup, seed);
    this.generation += 1;
  }

  restore(parsedSlot: unknown, loopHooks: WasmRestoreLoopHooks): unknown {
    const [, wasm] = this.requireHandle();
    const restoredSnapshot = restoreWasmSession({
      currentHandle: () => this.handle,
      replaceHandle: (handle) => { this.handle = handle; },
      restore: () => wasm.restore_json(JSON.stringify(parsedSlot)),
      snapshot: wasm.snapshot,
      drop: wasm.drop_session,
      ...loopHooks,
    });
    // 旧 handle 的 drop 成功后才推进；prepare 失败已属于新 generation。
    this.generation += 1;
    if (this.handle === null) throw new Error("WASM restore 没有安装新会话句柄");
    this.preparePublicBaseline(this.handle, wasm);
    return restoredSnapshot;
  }

  prepareBaseline(): void {
    const [handle, wasm] = this.requireHandle();
    this.preparePublicBaseline(handle, wasm);
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
