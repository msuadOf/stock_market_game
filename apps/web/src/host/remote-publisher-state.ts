import type { HostUpdate } from "./host-update.ts";

type Baseline = Extract<HostUpdate, { type: "baseline" }>;
type BaselineWaiter = { readonly resolve: () => void; readonly reject: (error: Error) => void };
export type RemoteQueryCursor = { readonly generation: string; readonly epoch: number };

/** Remote Publisher 消费端的连接身份、单槽 waiter 与基线缓存。 */
export class RemotePublisherState {
  private socket: WebSocket | null = null;
  private connectionGeneration = 0;
  private awaitingBaseline = true;
  private baselineWaiter: BaselineWaiter | null = null;
  private cachedBaseline: Baseline | null = null;
  private baselineEpoch = 0;

  currentSocket(): WebSocket | null { return this.socket; }
  baselineForRead(): Baseline | null { return this.cachedBaseline; }
  isAwaitingBaseline(): boolean { return this.awaitingBaseline; }
  hasBaselineWaiter(): boolean { return this.baselineWaiter !== null; }
  invalidateConnection(): number { return ++this.connectionGeneration; }
  isCurrentConnection(identity: number): boolean { return identity === this.connectionGeneration; }
  attachSocket(next: WebSocket): void { this.socket = next; }
  clearSocketIfCurrent(next: WebSocket): void { if (this.socket === next) this.socket = null; }

  detachSocket(): WebSocket | null {
    const current = this.socket;
    this.socket = null;
    return current;
  }

  beginResync(): void { this.awaitingBaseline = true; }

  beginBaselineWait(waiter: BaselineWaiter): void {
    this.rejectBaselineWaiter(new Error("远程基线请求已被替换，确认中断，结果未知"));
    this.baselineWaiter = waiter;
  }

  takeBaselineWaiter(): BaselineWaiter | null {
    const waiter = this.baselineWaiter;
    this.baselineWaiter = null;
    return waiter;
  }

  resolveBaselineWaiter(): void {
    this.takeBaselineWaiter()?.resolve();
  }

  rejectBaselineWaiter(error: Error): void {
    this.takeBaselineWaiter()?.reject(error);
  }

  installBaseline(update: Baseline, onTimelineChanged: () => void): void {
    this.baselineEpoch += 1;
    if (this.cachedBaseline !== null && this.cachedBaseline.generation !== update.generation) onTimelineChanged();
    this.cachedBaseline = update;
    this.awaitingBaseline = false;
  }

  isProtocolReady(): boolean { return !this.awaitingBaseline && this.cachedBaseline !== null; }
  isProtocolDeliverable(generation: string): boolean { return this.isProtocolReady() && this.hasGeneration(generation); }
  hasGeneration(generation: string): boolean { return this.cachedBaseline?.generation === generation; }

  captureQueryCursor(message: string): RemoteQueryCursor {
    if (this.cachedBaseline === null) throw new Error(message);
    return { generation: this.cachedBaseline.generation, epoch: this.baselineEpoch };
  }

  assertQueryCursor(cursor: RemoteQueryCursor, responseGeneration: unknown, message: string): void {
    if (responseGeneration !== cursor.generation || !this.hasGeneration(cursor.generation) || this.baselineEpoch !== cursor.epoch) throw new Error(message);
  }
}
