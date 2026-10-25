/**
 * 子 Worker 关闭全序（R4a 阶段 2，设计 §10.2.2）。
 *
 * 有限闭包 worker（G+N 组/部）的确定性终点 W1→W6：
 *
 * | 步 | 动作 | 所在侧 |
 * |----|------|--------|
 * | W1 | 闭包返回（确认无后续 Rust 工作） | Worker（Rust trampoline） |
 * | W2 | join 槽写 result + CAS WAITING→DONE + notify | Worker（Rust） |
 * | W3 | postMessage(workerExit) | Worker → 父队列（必达） |
 * | W4 | 置 bootstrap `rust_done` 标（迟到消息守卫） | Worker（本模块） |
 * | W5 | `self.close()`（专用 Worker 空事件循环不自动终止） | Worker（本模块） |
 * | W6 | trampoline JS 返回（事件循环已 closing） | Worker |
 * | S  | 收 workerExit → manifest 摘除 → 槽回收三门 | 主线程（facade-supervisor） |
 *
 * **迟到消息守卫**：W5 只丢弃**未派发**任务；已派发的 onmessage 会跑完。
 * W4 置标后 bootstrap 的 onmessage 一律丢弃，保证 close 后零 Rust 执行。
 *
 * deferred 常驻 worker **不适用** self.close()（常驻 recv 循环，唯一终点
 * = 实例废弃 terminate）——本模块只服务有限闭包 worker。
 */

/** 子 Worker 侧的最小 bootstrap 面（DedicatedWorkerGlobalScope 子集）。 */
export interface ChildBootstrapScope {
  readonly postMessage: (message: unknown) => void;
  readonly close: () => void;
}

/**
 * Worker 侧关闭器：W3/W4/W5 的确定性序。调用方在 trampoline 返回后调用；
 * 返回后不应再有任何 Rust 调用进入本 Worker。
 */
export class ChildShutdown {
  private rustDone = false;
  private closed = false;
  private readonly scope: ChildBootstrapScope;

  constructor(scope: ChildBootstrapScope) {
    this.scope = scope;
  }

  /** 是否已进入收尾（bootstrap onmessage 守卫判据）。 */
  isRustDone(): boolean {
    return this.rustDone;
  }

  isClosed(): boolean {
    return this.closed;
  }

  /**
   * W3→W5。先 post workerExit（父队列必达，不受本 Worker 后续 close 影响），
   * 再置 rust_done 标，最后 self.close()。重复调用是显式错误（协议违约）。
   */
  shutdown(workerId: number, slot: number, gen: number): void {
    if (this.rustDone || this.closed) {
      throw new Error(
        `facade 子 Worker ${workerId} 重复关闭（槽 ${slot}，gen ${gen}）——trampoline 只允许一个出口`,
      );
    }
    // W3：入父侧队列（必达）。字段与 Rust facade_worker_exit_notify 语义一致。
    this.scope.postMessage({ type: "facadeWorkerExit", workerId, slot, gen });
    // W4：迟到消息守卫（close 恰好派发中的任务会在 onmessage 开头见标即弃）。
    this.rustDone = true;
    // W5：确定性终点（专用 Worker 空事件循环不自动终止）。
    this.closed = true;
    this.scope.close();
  }

  /**
   * bootstrap onmessage 入口守卫：见 rust_done 标即丢弃（不进 Rust）。
   * 返回 true = 应当丢弃该消息。
   */
  shouldDropMessage(): boolean {
    return this.rustDone;
  }
}

/** 主线程侧 workerExit 处理（S 步；与 facade-supervisor 协作）。 */
export interface WorkerExitMessage {
  readonly type: "facadeWorkerExit";
  readonly workerId: number;
  readonly slot: number;
  readonly gen: number;
}

/** 消息形状守卫（跨 postMessage 边界不信任）。 */
export function parseWorkerExit(value: unknown): WorkerExitMessage | null {
  if (value === null || typeof value !== "object") return null;
  const source = value as Readonly<Record<string, unknown>>;
  if (source.type !== "facadeWorkerExit") return null;
  const { workerId, slot, gen } = source;
  if (!Number.isInteger(workerId) || !Number.isInteger(slot) || !Number.isInteger(gen)) return null;
  return { type: "facadeWorkerExit", workerId: workerId as number, slot: slot as number, gen: gen as number };
}
