export class DayEndPersistence {
  private generation: string | null = null;
  private epoch = 0;
  private tail: Promise<void> = Promise.resolve();
  private pendingWrite: Promise<boolean> | null = null;

  install(generation: string): void {
    this.epoch += 1;
    this.generation = generation;
  }

  invalidate(): void {
    this.epoch += 1;
    this.generation = null;
  }

  idle(): Promise<void> {
    return this.tail;
  }

  /** 等待调用前已提交的写入；本次等待的错误必须交给读档入口显示。 */
  async beforeRead(): Promise<void> {
    const pending = this.pendingWrite;
    await this.tail;
    if (pending !== null) await pending;
  }

  completed(
    generation: string,
    captured: Promise<unknown>,
    write: (slot: unknown, isCurrent: () => boolean) => Promise<void | boolean>,
  ): Promise<boolean> {
    const epoch = this.epoch;
    const isCurrent = () => this.epoch === epoch && this.generation === generation;
    const candidate = captured.then((slot) => ({ slot }), (error: unknown) => ({ error }));
    const operation = this.tail.then(async () => {
      const result = await candidate;
      if (!isCurrent()) return false;
      if ("error" in result) throw result.error;
      const committed = await write(result.slot, isCurrent);
      return committed !== false && isCurrent();
    });
    this.pendingWrite = operation;
    const finish = () => { if (this.pendingWrite === operation) this.pendingWrite = null; };
    this.tail = operation.then(finish, finish);
    return operation;
  }
}
