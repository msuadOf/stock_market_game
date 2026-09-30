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
