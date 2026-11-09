import type { DayEndExportReference, DayEndExportSubscription } from "./engine-host.ts";

type Confirmation = { readonly subscription: string; readonly afterSeq: number };

type Cursor = { readonly generation: string; readonly connection: object };
type Control = {
  readonly kind: "subscribe" | "unsubscribe";
  readonly cursor: Cursor;
  readonly resolve: (subscription?: Confirmation) => void;
  readonly reject: (error: Error) => void;
};
type Lease = { readonly subscription: string; readonly cursor: Cursor; readonly onFailure: (error: Error) => void; active: boolean };
type Ports = {
  nextRequestId(): number;
  current(): Cursor;
  send(value: Record<string, Record<string, unknown>>): void;
  request(path: string, body: unknown): Promise<unknown>;
  interruptConnection(error: Error, connection: object): void;
};

/** 文件导出独立于市场命令确认；中断只撤销本连接的订阅，调用方负责显式展示并重新授权。 */
export class RemoteDayEndExports {
  private readonly pending = new Map<number, Control>();
  private readonly interrupted = new Set<number>();
  private lease: Lease | null = null;
  private readonly ports: Ports;
  constructor(ports: Ports) { this.ports = ports; }

  private assertCursor(cursor: Cursor): void {
    const current = this.ports.current();
    if (current.generation !== cursor.generation || current.connection !== cursor.connection) throw new Error("日终文件导出确认属于已过期 connection/generation");
  }

  private control(kind: Control["kind"], cursor: Cursor, subscription?: string): Promise<Confirmation | undefined> {
    const id = this.ports.nextRequestId();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id); this.interrupted.add(id);
        const error = new Error("日终文件导出订阅确认超时（5000ms），请重新授权文件");
        reject(error);
        // subscription token 可能已在服务端创建；关闭原连接才能确定释放未知租约。
        this.ports.interruptConnection(error, cursor.connection);
      }, 5000);
      this.pending.set(id, { kind, cursor, resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
      try {
        this.ports.send({ [kind === "subscribe" ? "SubscribeDayEndExports" : "UnsubscribeDayEndExports"]: {
          request_id: id, generation: cursor.generation, ...(subscription === undefined ? {} : { subscription }),
        } });
      } catch (error) {
        this.pending.delete(id); clearTimeout(timer);
        reject(error instanceof Error ? error : new Error(String(error)));
      }
    });
  }

  async subscribe(onFailure: Lease["onFailure"]): Promise<DayEndExportSubscription> {
    if (this.lease !== null || [...this.pending.values()].some(control => control.kind === "subscribe")) throw new Error("日终文件导出已有活动或待确认订阅，请先关闭旧目标");
    const cursor = this.ports.current();
    const confirmation = await this.control("subscribe", cursor);
    this.assertCursor(cursor);
    if (confirmation === undefined) throw new Error("日终文件导出订阅确认缺少 subscription");
    const { subscription, afterSeq } = confirmation;
    const lease: Lease = { subscription, cursor, onFailure, active: true };
    this.lease = lease;
    const assertActive = () => {
      if (!lease.active) throw new Error("日终文件导出订阅已失效，请重新授权文件");
      this.assertCursor(cursor);
    };
    const body = (candidate: DayEndExportReference) => ({ generation: cursor.generation, subscription, candidate });
    return {
      afterSeq,
      read: async candidate => {
        assertActive();
        const value = await this.ports.request("/api/day-end-export", body(candidate));
        assertActive();
        return value;
      },
      acknowledge: async candidate => {
        // 已关闭的租约已由服务端取消清理，无需向新连接确认旧候选。
        if (!lease.active) return;
        assertActive();
        const value = await this.ports.request("/api/day-end-export/ack", body(candidate));
        if (value !== null) throw new Error("日终文件导出 ACK 响应必须为空（HTTP 204）");
        assertActive();
      },
      close: async () => {
        if (!lease.active) return;
        lease.active = false;
        if (this.lease === lease) this.lease = null;
        this.assertCursor(cursor);
        try { await this.control("unsubscribe", cursor, subscription); }
        catch (error) {
          const failure = error instanceof Error ? error : new Error(String(error));
          this.ports.interruptConnection(failure, cursor.connection);
          throw failure;
        }
      },
    };
  }

  subscribed(id: number, subscription: string, afterSeq: number): void { this.confirm(id, "subscribe", { subscription, afterSeq }); }
  unsubscribed(id: number): void { this.confirm(id, "unsubscribe"); }
  private confirm(id: number, kind: Control["kind"], subscription?: Confirmation): void {
    if (this.interrupted.delete(id)) return;
    const pending = this.pending.get(id);
    if (pending === undefined || pending.kind !== kind) throw new Error(`未知日终文件导出确认 ${id}`);
    this.pending.delete(id);
    try { this.assertCursor(pending.cursor); pending.resolve(subscription); }
    catch (error) { pending.reject(error instanceof Error ? error : new Error(String(error))); }
  }

  rejectGateway(id: number, error: Error): boolean {
    if (this.interrupted.delete(id)) return true;
    const pending = this.pending.get(id);
    if (pending === undefined) return false;
    this.pending.delete(id); pending.reject(error); return true;
  }

  failed(subscription: string, code: string, message: string): void {
    const lease = this.lease;
    if (lease === null || lease.subscription !== subscription) return;
    this.cancelLease(new Error(`${code}: ${message}`));
  }
  private cancelLease(error: Error): void {
    const lease = this.lease;
    this.lease = null;
    if (lease !== null) { lease.active = false; lease.onFailure(error); }
  }
  hasSubscriptions(): boolean { return this.lease !== null || this.pending.size !== 0; }
  interrupt(error: Error): void {
    for (const [id, pending] of this.pending) { this.interrupted.add(id); pending.reject(error); }
    this.pending.clear(); this.cancelLease(error);
  }
  invalidateGeneration(generation: string): void {
    if ((this.lease !== null && this.lease.cursor.generation !== generation)
      || [...this.pending.values()].some(control => control.cursor.generation !== generation)) {
      this.interrupt(new Error("日终文件导出 generation 已变化，请重新授权文件"));
    }
  }
}
