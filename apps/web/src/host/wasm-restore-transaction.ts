import { describeWasmFailure } from "./wasm-failure.ts";

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
  let primaryFailed = false;
  let primaryError: unknown;
  let restoredSnapshot: TSnapshot | undefined;
  try {
    candidate = transaction.restore();
    restoredSnapshot = transaction.snapshot(candidate);
    const previous = transaction.currentHandle();
    transaction.replaceHandle(candidate);
    candidate = null;
    if (previous !== null) transaction.drop(previous);
  } catch (error) {
    primaryFailed = true;
    primaryError = error;
  }
  const cleanupErrors: unknown[] = [];
  if (candidate !== null) {
    try { transaction.drop(candidate); }
    catch (error) { cleanupErrors.push(error); }
  }
  if (transaction.wasRunning) {
    try { transaction.restart(); }
    catch (error) { cleanupErrors.push(error); }
  }
  if (cleanupErrors.length > 0) {
    const errors = primaryFailed ? [primaryError, ...cleanupErrors] : cleanupErrors;
    if (errors.length === 1) throw errors[0];
    throw new AggregateError(errors, errors.map(error => describeWasmFailure(error)).join("；"), { cause: primaryFailed ? primaryError : errors[0] });
  }
  if (primaryFailed) throw primaryError;
  // 成功分支已执行 snapshot；TSnapshot 本身可以合法包含 undefined。
  return restoredSnapshot as TSnapshot;
}
