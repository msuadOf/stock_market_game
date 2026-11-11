export class InitialSaveSource<Value> {
  private consumed = false;
  private explicitlySelected = false;
  private selectionController = new AbortController();
  private frozenSelection: symbol | undefined;
  private selectionChange: symbol | undefined;
  private readonly activeReads = new Set<Promise<void>>();
  private releaseFailure: AggregateError | undefined;

  /** 读取可能持有 decoder/reader；跨选择后仍须等待其真实清理。 */
  runSelectionRead<Result>(load: (signal: AbortSignal) => Promise<Result>): Promise<Result> {
    const signal = this.selectionController.signal;
    const pending = Promise.resolve().then(() => { signal.throwIfAborted(); return load(signal); });
    const settled = pending.then(
      () => { this.activeReads.delete(settled); },
      error => {
        this.activeReads.delete(settled);
        // reader 将清理失败包装为 AggregateError；纯取消及 schema 拒绝不污染释放状态。
        if (error instanceof AggregateError && error !== signal.reason) {
          this.releaseFailure = this.releaseFailure === undefined ? error
            : new AggregateError([this.releaseFailure, error], "多个存档读取释放失败");
        }
      },
    );
    this.activeReads.add(settled);
    return pending;
  }

  readReleaseFailure(): AggregateError | undefined { return this.releaseFailure; }

  async waitReadRelease(): Promise<void> {
    while (this.activeReads.size > 0) await Promise.all([...this.activeReads]);
    if (this.releaseFailure !== undefined) throw this.releaseFailure;
  }

  selectionSignal(): AbortSignal { return this.selectionController.signal; }

  /** 冻结交接期间不接收新目标；调用方在未发布时释放，已发布则保持至页面重载。 */
  freezeSelection(signal: AbortSignal): () => void {
    if (signal !== this.selectionController.signal || signal.aborted) throw new Error("存档选择已更换，不能冻结旧目标");
    if (this.selectionChange !== undefined) throw new Error("存档选择正在提交，不能冻结旧目标");
    if (this.frozenSelection !== undefined) throw new Error("存档选择正在重载交接，请完成重载后重试");
    const token = Symbol("重载选择"); this.frozenSelection = token;
    return () => { if (this.frozenSelection === token) this.frozenSelection = undefined; };
  }

  isSelectionFrozen(): boolean { return this.frozenSelection !== undefined; }
  beginSelectionChange(): () => void {
    this.assertSelectable();
    if (this.selectionChange !== undefined) throw new Error("存档选择正在提交");
    const token = Symbol("选择提交"); this.selectionChange = token;
    return () => { if (this.selectionChange === token) this.selectionChange = undefined; };
  }
  assertSelectable(): void {
    if (this.frozenSelection !== undefined) throw new Error("存档选择正在重载交接，请完成重载后重试");
  }

  hasExplicitSelection(): boolean { return this.explicitlySelected; }
  private pending: Promise<Value | null> | null = null;

  read(load: (signal: AbortSignal) => Promise<Value | null>): Promise<Value | null> {
    if (this.consumed) return Promise.resolve(null);
    const signal = this.selectionController.signal;
    this.pending ??= Promise.resolve().then(() => { signal.throwIfAborted(); return load(signal); });
    return this.pending;
  }

  complete(): void {
    this.assertSelectable();
    this.selectionController.abort(new Error("初始存档选择已结束"));
    this.selectionController = new AbortController();
    this.explicitlySelected = false;
    this.consumed = true;
    this.pending = null;
  }

  select(value: Value): void {
    this.assertSelectable();
    this.selectionController.abort(new Error("初始存档选择已替换"));
    this.selectionController = new AbortController();
    this.explicitlySelected = true;
    this.consumed = false;
    this.pending = Promise.resolve(value);
  }

  reset(): void {
    this.complete();
    this.explicitlySelected = true;
  }
}

export class SessionReplacementGate {
  private epoch = 0;
  private busy = false;

  begin(): number | null {
    if (this.busy) return null;
    this.busy = true;
    return ++this.epoch;
  }

  isCurrent(epoch: number): boolean {
    return this.busy && epoch === this.epoch;
  }

  finish(epoch: number): void {
    if (this.isCurrent(epoch)) this.busy = false;
  }

  invalidate(): void {
    this.epoch += 1;
    this.busy = false;
  }
}

export type BaselineSyncResult = { readonly kind: "current" | "stale" } | { readonly kind: "failed"; readonly error: unknown };

export async function synchronizeCurrentBaseline(sync: () => Promise<void>, isCurrent: () => boolean): Promise<BaselineSyncResult> {
  if (!isCurrent()) return { kind: "stale" };
  try {
    await sync();
    return { kind: isCurrent() ? "current" : "stale" };
  } catch (error) {
    return isCurrent() ? { kind: "failed", error } : { kind: "stale" };
  }
}

/** 展开清理链，避免 UI 只显示 AggregateError 的顶层说明。 */
export function describeInitialSaveReadFailure(error: unknown, seen = new Set<unknown>()): string {
  if (seen.has(error)) return "错误详情存在循环引用";
  seen.add(error);
  const message = error instanceof Error ? error.message : String(error);
  return error instanceof AggregateError
    ? `${message}：${error.errors.map(cause => describeInitialSaveReadFailure(cause, seen)).join("；")}` : message;
}
