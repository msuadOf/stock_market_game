import { CheckpointCloseError } from "./wasm-checkpoint-client.ts";

export interface ArchiveShutdownFailure { readonly kind: "nondurable"; readonly message: string; readonly workerQuiescence: "confirmed"; readonly ownerConsumed: true; readonly memoryReusable: false }

/** 物理退出与耐久是两件事；未知退出禁止在共享 Memory 上继续析构。 */
export async function closeCheckpointAndCleanup<T>(close: () => Promise<void> | undefined, cleanup: () => T): Promise<{ value: T; archiveFailure?: ArchiveShutdownFailure }> {
  let archiveError: CheckpointCloseError | undefined;
  try { await close(); }
  catch (error) {
    if (!(error instanceof CheckpointCloseError) || error.quiescence !== "confirmed") throw error;
    archiveError = error;
  }
  let value: T;
  try { value = cleanup(); }
  catch (error) {
    if (archiveError === undefined) throw error;
    throw new AggregateError([archiveError, error], "归档未耐久且后续会话清理失败", { cause: archiveError });
  }
  return archiveError === undefined ? { value } : { value, archiveFailure: { kind: "nondurable", message: archiveError.message, workerQuiescence: "confirmed", ownerConsumed: true, memoryReusable: false } };
}

export function readArchiveShutdownFailure(response: Readonly<Record<string, unknown>>): ArchiveShutdownFailure | undefined {
  const expected = Object.hasOwn(response, "archiveFailure") ? "archiveFailure,generation,report,requestId,type" : "generation,report,requestId,type";
  if (Object.keys(response).sort().join(",") !== expected) throw new Error("shutdownCompleted 字段无效");
  if (!Object.hasOwn(response, "archiveFailure")) return;
  const value = response.archiveFailure;
  if (value === null || typeof value !== "object" || Object.keys(value).sort().join(",") !== "kind,memoryReusable,message,ownerConsumed,workerQuiescence" || !("kind" in value) || value.kind !== "nondurable" || !("message" in value) || typeof value.message !== "string" || !("workerQuiescence" in value) || value.workerQuiescence !== "confirmed" || !("ownerConsumed" in value) || value.ownerConsumed !== true || !("memoryReusable" in value) || value.memoryReusable !== false) throw new Error("归档关停失败报告无效");
  return { kind: "nondurable", message: value.message, workerQuiescence: "confirmed", ownerConsumed: true, memoryReusable: false };
}
