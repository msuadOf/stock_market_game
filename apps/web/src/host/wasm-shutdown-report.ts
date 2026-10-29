export interface WasmShutdownIssue { readonly stage: string; readonly worker: string | null; readonly detail: string }
export interface WasmShutdownWorker { readonly worker: string; readonly result: { readonly kind: "ok" } | { readonly kind: "error"; readonly message: string } }
export interface WasmShutdownCompletion {
  readonly ownedWorkers: number; readonly joinedWorkers: number; readonly workers: readonly WasmShutdownWorker[];
  readonly groupFinals: number; readonly deskFinals: number;
}
export type WasmSessionShutdown =
  | { readonly kind: "success"; readonly handle: number; readonly runtime: WasmShutdownCompletion | null }
  | { readonly kind: "failure"; readonly handle: number; readonly primary: WasmShutdownIssue; readonly secondary: readonly WasmShutdownIssue[]; readonly completion: WasmShutdownCompletion | null };

/** outcome 区分 FFI 正常返回与抛出的失败值；不依赖错误字符串猜分支。 */
export function parseRustSessionShutdown(value: unknown, outcome: "success" | "failure", expectedHandle: number): WasmSessionShutdown {
  const wire = exact(value, ["handle", "result"], "shutdown reply");
  const handle = count(wire.handle, "handle");
  if (handle === 0 || handle > 0xffff_ffff || handle !== expectedHandle) throw new Error("shutdown handle 与当前 owner 不符");
  if (outcome === "success") {
    const report = exact(wire.result, ["runtime"], "shutdown report");
    return { kind: "success", handle, runtime: report.runtime === undefined ? null : completion(report.runtime, true) };
  }
  if (outcome !== "failure") throw new Error("shutdown outcome 必须显式区分成功/失败");
  const report = exact(wire.result, ["primary", "secondary", "completion"], "shutdown failure");
  if (!Array.isArray(report.secondary)) throw new Error("shutdown secondary 必须为数组");
  return { kind: "failure", handle, primary: issue(report.primary), secondary: report.secondary.map(issue),
    completion: report.completion === undefined ? null : completion(report.completion, false) };
}

function exact(value: unknown, keys: readonly string[], where: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error(`${where} 必须为对象`);
  const source = value as Record<string, unknown>;
  if (Object.keys(source).length !== keys.length || keys.some((key) => !Object.hasOwn(source, key))) throw new Error(`${where} 字段不完整或存在未知字段`);
  return source;
}
function text(value: unknown, where: string): string {
  if (typeof value !== "string" || value.trim().length === 0) throw new Error(`${where} 必须为非空字符串`);
  return value;
}
function count(value: unknown, where: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw new Error(`${where} 必须为非负 safe integer`);
  return value;
}
function worker(value: unknown): string {
  const name = text(value, "worker");
  if (!/^(group|desk)-(0|[1-9]\d*)$/.test(name)) throw new Error("shutdown worker 身份非法");
  return name;
}
function issue(value: unknown): WasmShutdownIssue {
  const source = exact(value, ["stage", "worker", "detail"], "shutdown issue");
  return { stage: text(source.stage, "stage"), worker: source.worker === undefined ? null : worker(source.worker), detail: text(source.detail, "detail") };
}
function completion(value: unknown, success: boolean): WasmShutdownCompletion {
  const source = exact(value, ["owned_workers", "joined_workers", "worker_results", "group_finals", "desk_finals"], "shutdown completion");
  const ownedWorkers = count(source.owned_workers, "owned_workers"), joinedWorkers = count(source.joined_workers, "joined_workers");
  const groupFinals = count(source.group_finals, "group_finals"), deskFinals = count(source.desk_finals, "desk_finals");
  if (!Array.isArray(source.worker_results)) throw new Error("worker_results 必须为数组");
  const workers: WasmShutdownWorker[] = source.worker_results.map((value) => {
    const entry = exact(value, ["worker", "result"], "worker result");
    const identity = worker(entry.worker), result = entry.result;
    if (result !== null && typeof result === "object" && !Array.isArray(result)) {
      const variant = result as Record<string, unknown>;
      if (Object.keys(variant).length === 1) {
        if (Object.hasOwn(variant, "Ok") && variant.Ok === undefined) return { worker: identity, result: { kind: "ok" } };
        if (Object.hasOwn(variant, "Err")) return { worker: identity, result: { kind: "error", message: text(variant.Err, "worker Err") } };
      }
    }
    throw new Error("worker result 必须为真实 Rust Ok(()) 或 Err(String)");
  });
  if (new Set(workers.map((entry) => entry.worker)).size !== workers.length || workers.length !== joinedWorkers || joinedWorkers > ownedWorkers) throw new Error("shutdown join 数量或 worker 唯一性不符");
  if (groupFinals + deskFinals > ownedWorkers) throw new Error("shutdown Final 数量超出 owned worker");
  if (success && (joinedWorkers !== ownedWorkers || workers.some((entry) => entry.result.kind !== "ok")
    || groupFinals !== workers.filter((entry) => entry.worker.startsWith("group-")).length
    || deskFinals !== workers.filter((entry) => entry.worker.startsWith("desk-")).length)) throw new Error("shutdown 成功必须含全部真实成功 join 与 Final");
  return { ownedWorkers, joinedWorkers, workers, groupFinals, deskFinals };
}

/** Worker 传回的是已归一的纯数据；主线程仍独立检查消息边界。 */
export function parseWasmSessionShutdown(value: unknown): WasmSessionShutdown {
  if (value === null || typeof value !== "object") throw new Error("shutdown report 必须是对象");
  const kind = (value as Record<string, unknown>).kind;
  const source = exact(value, kind === "success" ? ["kind", "handle", "runtime"] : ["kind", "handle", "primary", "secondary", "completion"], "Worker shutdown report");
  if (kind !== "success" && kind !== "failure") throw new Error("shutdown report kind 非法");
  const handle = count(source.handle, "handle");
  function wireIssue(value: unknown): unknown {
    const item = exact(value, ["stage", "worker", "detail"], "Worker shutdown issue");
    return { ...item, worker: item.worker === null ? undefined : item.worker };
  }
  function wireCompletion(value: unknown): unknown {
    if (value === null) return undefined;
    const item = exact(value, ["ownedWorkers", "joinedWorkers", "workers", "groupFinals", "deskFinals"], "Worker shutdown completion");
    if (!Array.isArray(item.workers)) throw new Error("Worker shutdown workers 必须为数组");
    return { owned_workers: item.ownedWorkers, joined_workers: item.joinedWorkers, group_finals: item.groupFinals, desk_finals: item.deskFinals,
      worker_results: item.workers.map((entry) => {
        const result = exact(entry, ["worker", "result"], "Worker shutdown result");
        const ok = result.result !== null && typeof result.result === "object" && (result.result as Record<string, unknown>).kind === "ok";
        const variant = exact(result.result, ok ? ["kind"] : ["kind", "message"], "Worker shutdown variant");
        if (variant.kind !== "ok" && variant.kind !== "error") throw new Error("Worker shutdown variant 非法");
        return { worker: result.worker, result: ok ? { Ok: undefined } : { Err: variant.message } };
      }) };
  }
  if (kind === "success") return parseRustSessionShutdown({ handle, result: { runtime: wireCompletion(source.runtime) } }, kind, handle);
  if (!Array.isArray(source.secondary)) throw new Error("Worker shutdown secondary 必须为数组");
  return parseRustSessionShutdown({ handle, result: { primary: wireIssue(source.primary), secondary: source.secondary.map(wireIssue), completion: wireCompletion(source.completion) } }, kind, handle);
}
