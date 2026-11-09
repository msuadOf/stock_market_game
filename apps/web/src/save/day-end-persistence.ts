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
    const candidate = captured.then((slot) => ({ slot }), (error: unknown) => ({ error }));
    return this.enqueue(generation, async isCurrent => {
      const result = await candidate;
      if (!isCurrent()) return false;
      if ("error" in result) throw result.error;
      const committed = await write(result.slot, isCurrent);
      return committed !== false && isCurrent();
    });
  }

  /** remote 候选保留于宿主；队列轮到该日后才读取，ACK 完成前不开始下一日。 */
  completedDeferred(
    generation: string,
    capture: () => Promise<unknown>,
    write: (slot: unknown, isCurrent: () => boolean) => Promise<void | boolean>,
    release: () => Promise<void>,
    isEligible: () => boolean,
  ): Promise<boolean> {
    return this.enqueue(generation, async queueIsCurrent => {
      const isCurrent = () => queueIsCurrent() && isEligible();
      let failure: { error: unknown } | undefined;
      let committed = false;
      try {
        if (isCurrent()) {
          const slot = await capture();
          if (isCurrent()) committed = (await write(slot, isCurrent)) !== false;
        }
      } catch (error) { failure = { error }; }
      try { await release(); }
      catch (error) {
        if (failure !== undefined) throw new AggregateError([failure.error, error], `日终文件输出与候选 ACK 均失败：${String(failure.error)}；${String(error)}`, { cause: failure.error });
        throw error;
      }
      if (failure !== undefined) throw failure.error;
      return committed && isCurrent();
    });
  }

  private enqueue(generation: string, execute: (isCurrent: () => boolean) => Promise<boolean>): Promise<boolean> {
    const epoch = this.epoch;
    const isCurrent = () => this.epoch === epoch && this.generation === generation;
    const operation = this.tail.then(() => execute(isCurrent));
    this.pendingWrite = operation;
    const finish = () => { if (this.pendingWrite === operation) this.pendingWrite = null; };
    this.tail = operation.then(finish, finish);
    return operation;
  }
}
