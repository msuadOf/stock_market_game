export interface WasmRestoreTransaction<TSnapshot> {
  readonly currentHandle: () => number | null;
  readonly replaceHandle: (handle: number) => void;
  readonly restore: () => number;
  readonly snapshot: (handle: number) => TSnapshot;
  readonly drop: (handle: number) => void;
  readonly wasRunning: boolean;
  readonly stop: () => void;
  readonly restart: () => void;
}

export function restoreWasmSession<TSnapshot>(
  transaction: WasmRestoreTransaction<TSnapshot>,
): TSnapshot {
  if (transaction.wasRunning) transaction.stop();
  let candidate: number | null = null;
  try {
    candidate = transaction.restore();
    const restoredSnapshot = transaction.snapshot(candidate);
    const previous = transaction.currentHandle();
    transaction.replaceHandle(candidate);
    candidate = null;
    if (previous !== null) transaction.drop(previous);
    return restoredSnapshot;
  } finally {
    if (candidate !== null) transaction.drop(candidate);
    if (transaction.wasRunning) transaction.restart();
  }
}
