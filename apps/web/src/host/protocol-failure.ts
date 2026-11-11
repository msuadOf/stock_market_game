import type { HostFailure } from "./host-update.ts";
import { exact, field, record, safeU32, values } from "./protocol/guards.ts";
import { ProtocolError } from "./protocol/types.ts";

/** 只消费可信 Host 本次真实 prepare 拒绝的专属契约；普通 recoverable 不授予继续写权限。 */
export function isReportCorrectionPrepareRejection(failure: HostFailure, host: "server" | "desktop", generation: string): boolean {
  if (failure.code !== "REPORT_CORRECTION_PREPARE_REJECTED" || failure.where !== `${host}.actor.prepare_civil_updates` || failure.recoverable !== true) return false;
  if (failure.context === null || typeof failure.context !== "object" || Array.isArray(failure.context)) return false;
  const context = failure.context as Record<string, unknown>;
  const fields = ["operation", "tick", "seq", "day", "generation"];
  if (Object.keys(context).length !== fields.length || !fields.every(field => Object.hasOwn(context, field))) return false;
  if (context.operation !== "endCivilDay" || context.generation !== generation) return false;
  if (![context.tick, context.seq].every(value => Number.isSafeInteger(value) && Number(value) >= 0)) return false;
  if (!Number.isSafeInteger(context.day) || Number(context.day) < 0 || Number(context.day) > 4_294_967_295) return false;

  const seen = new Set<object>();
  let cause: unknown = failure.cause;
  while (cause !== null) {
    if (typeof cause !== "object" || Array.isArray(cause) || seen.has(cause)) return false;
    seen.add(cause);
    const item = cause as Record<string, unknown>;
    const causeFields = Object.keys(item);
    if (causeFields.length !== 3 || !["code", "message", "cause"].every(field => Object.hasOwn(item, field))) return false;
    if (typeof item.code !== "string" || item.code.trim().length === 0) return false;
    if (typeof item.message !== "string" || item.message.trim().length === 0) return false;
    if (seen.size === 1 && item.code !== "REPORT_CORRECTION_REJECTED") return false;
    cause = item.cause;
  }
  return seen.size > 0;
}

export function parseHostFailure(value: unknown, transportWhere?: string): HostFailure {
  const path = "HostFailure";
  const source = record(value, path);
  const allowed = ["code", "where", "message", "stack", "cause", "context", "recoverable", "recoveryActions"];
  if (Object.keys(source).some((key) => !allowed.includes(key))) {
    throw new ProtocolError("PROTOCOL_MALFORMED", path, "HostFailure 包含未知字段");
  }
  function nonempty(value: unknown, key: string): string {
    if (typeof value !== "string" || value.trim().length === 0) {
      throw new ProtocolError("PROTOCOL_MALFORMED", `${path}.${key}`, `HostFailure.${key} 必须是非空字符串`);
    }
    return value;
  }
  const failure: HostFailure = {
    code: nonempty(source.code, "code"),
    where: nonempty(Object.hasOwn(source, "where") ? source.where : transportWhere, "where"),
    message: nonempty(source.message, "message"),
  };
  const details: {
    stack?: string;
    cause?: unknown;
    context?: unknown;
    recoverable?: boolean | null;
    recoveryActions?: readonly string[] | null;
  } = {};
  if (Object.hasOwn(source, "stack")) {
    if (typeof source.stack !== "string") throw new ProtocolError("PROTOCOL_MALFORMED", `${path}.stack`, "HostFailure.stack 必须是字符串");
    details.stack = source.stack;
  }
  for (const key of ["cause", "context"] as const) {
    if (Object.hasOwn(source, key)) details[key] = source[key];
  }
  if (Object.hasOwn(source, "recoverable")) {
    if (source.recoverable !== null && typeof source.recoverable !== "boolean") {
      throw new ProtocolError("PROTOCOL_MALFORMED", `${path}.recoverable`, "HostFailure.recoverable 必须是布尔值或 null（未知）");
    }
    details.recoverable = source.recoverable;
  }
  if (Object.hasOwn(source, "recoveryActions")) {
    if (source.recoveryActions === null) details.recoveryActions = null;
    else {
      if (!Array.isArray(source.recoveryActions)) {
        throw new ProtocolError("PROTOCOL_MALFORMED", `${path}.recoveryActions`, "HostFailure.recoveryActions 必须是建议文本数组或 null（未知）");
      }
      details.recoveryActions = source.recoveryActions.map((action) => nonempty(action, "recoveryActions"));
    }
  }
  if (failure.code === "CATALOG_BASELINE_FAILED") {
    const context = record(details.context, "HostFailure.context");
    const fault = record(field(context, "fault", "HostFailure.context"), "HostFailure.context.fault");
    exact(fault, ["code", "context"], "HostFailure.context.fault");
    if (fault.code !== 3019) throw new ProtocolError("PROTOCOL_MALFORMED", "HostFailure.context.fault.code", "目录故障必须使用3019");
    const slots = values(fault.context, "HostFailure.context.fault.context");
    if (slots.length !== 1) throw new ProtocolError("PROTOCOL_MALFORMED", "HostFailure.context.fault.context", "目录故障必须恰含一个 CompanyId");
    const company = safeU32(slots[0], "HostFailure.context.fault.context[0]");
    return { ...failure, ...details, message: `公司 CompanyId ${company} 的上市证券缺少于当前 StockCatalog，无法建立可信 baseline` };
  }
  return { ...failure, ...details };
}
