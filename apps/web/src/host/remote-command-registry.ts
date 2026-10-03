import type { HostFailure } from "./host-update.ts";

type CommandWaiter = { readonly resolve: () => void; readonly reject: (error: Error) => void };

/** CommandQueued 只确认入队；此处不判断委托受理或成交。 */
export class RemoteCommandRegistry {
  private requestSequence = 0;
  private readonly pendingCommands = new Map<number, CommandWaiter>();

  next(): number { return ++this.requestSequence; }

  register(id: number, waiter: CommandWaiter): void { this.pendingCommands.set(id, waiter); }

  resolveQueued(id: number): void {
    const pending = this.pendingCommands.get(id);
    if (pending === undefined) throw new Error(`收到未知写请求 ${id} 的入队确认`);
    this.pendingCommands.delete(id);
    pending.resolve();
  }

  rejectGateway(id: number, failure: HostFailure): void {
    const pending = this.pendingCommands.get(id);
    if (pending === undefined) throw new Error(`收到未知写请求 ${id} 的网关错误`);
    this.pendingCommands.delete(id);
    pending.reject(new Error(`${failure.code}: ${failure.message}`));
  }

  rejectAll(failure: HostFailure): void {
    for (const pending of this.pendingCommands.values()) pending.reject(new Error(`${failure.code}: ${failure.message}`));
    this.pendingCommands.clear();
  }
}
