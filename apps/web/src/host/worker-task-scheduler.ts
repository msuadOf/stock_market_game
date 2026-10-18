/** Worker task 调度器；即时 task 通过 MessageChannel yield，避免嵌套 timer clamp。 */
export class WorkerTaskScheduler {
  private readonly timers: {
    schedule: (callback: () => void, delay: number) => ReturnType<typeof setTimeout>;
    cancel: (timer: ReturnType<typeof setTimeout>) => void;
  };
  private readonly channel = new MessageChannel();
  private readonly pending = new Map<number, { callback: () => void; timer: ReturnType<typeof setTimeout> | null }>();
  private nextId = 0;
  private disposed = false;

  constructor(timers = {
    schedule: (callback: () => void, delay: number) => setTimeout(callback, delay),
    cancel: (timer: ReturnType<typeof setTimeout>) => clearTimeout(timer),
  }) {
    this.timers = timers;
    this.channel.port1.onmessage = event => this.run(Number(event.data));
  }

  schedule(callback: () => void, delay: number): number {
    if (this.disposed) throw new Error("Worker task 调度器已关闭");
    if (!Number.isFinite(delay) || delay < 0) throw new Error("Worker task delay 必须是非负有限数");
    if (!Number.isSafeInteger(this.nextId + 1)) throw new Error("Worker task identity 溢出");
    const id = ++this.nextId;
    const task = { callback, timer: null as ReturnType<typeof setTimeout> | null };
    this.pending.set(id, task);
    if (delay === 0) this.channel.port2.postMessage(id);
    else task.timer = this.timers.schedule(() => this.run(id), delay);
    return id;
  }

  private run(id: number): void {
    const task = this.pending.get(id);
    if (task === undefined) return; // 已取消的 task 仍可能留在浏览器消息队列。
    this.pending.delete(id);
    task.callback();
  }

  cancel(id: number): void {
    const task = this.pending.get(id);
    if (task === undefined) return;
    this.pending.delete(id);
    if (task.timer !== null) this.timers.cancel(task.timer);
  }

  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    for (const id of this.pending.keys()) this.cancel(id);
    this.channel.port1.close();
    this.channel.port2.close();
  }
}
