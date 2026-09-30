import type { HostFailure } from "./host-update.ts";
import { record } from "./protocol/guards.ts";
import { ProtocolError } from "./protocol/types.ts";

export function parseHostFailure(value: unknown, transportWhere?: string): HostFailure {
  const path = "HostFailure";
  const source = record(value, path);
  const allowed = ["code", "where", "message", "cause", "context", "recoverable", "recoveryActions"];
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
    cause?: unknown;
    context?: unknown;
    recoverable?: boolean | null;
    recoveryActions?: readonly string[] | null;
  } = {};
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
  return { ...failure, ...details };
}
