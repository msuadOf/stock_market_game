import type { HostFailure } from "./host-update.ts";

export type WasmFailureClassification =
  | { readonly kind: "structured"; readonly code: string; readonly message: string }
  | { readonly kind: "access-error"; readonly reason: string }
  | { readonly kind: "unstructured" };

function safeString(value: unknown): string {
  try {
    return String(value);
  } catch {
    return "<无法读取错误详情>";
  }
}

export function describeWasmFailure(error: unknown): string {
  if (typeof error === "string") return error;
  try {
    if (error instanceof Error) return safeString(error.message);
  } catch (inspectionError) {
    return `检查错误对象失败：${safeString(inspectionError)}`;
  }
  if (error !== null && typeof error === "object") {
    try {
      return `非标准错误对象：${JSON.stringify(error)}`;
    } catch (serializationError) {
      let reason: string;
      try {
        reason = serializationError instanceof Error ? safeString(serializationError.message) : safeString(serializationError);
      } catch (inspectionError) {
        reason = `检查序列化错误失败：${safeString(inspectionError)}`;
      }
      return `非标准错误对象无法序列化：${reason}`;
    }
  }
  return safeString(error);
}

export function classifyWasmFailure(error: unknown): WasmFailureClassification {
  if (error !== null && typeof error === "object") {
    try {
      if (Array.isArray(error)) return { kind: "unstructured" };
      const candidate = error as Readonly<Record<string, unknown>>;
      const code = candidate.code;
      const message = candidate.message;
      if (
        typeof code === "string"
        && code.length > 0
        && typeof message === "string"
        && message.length > 0
      ) {
        return { kind: "structured", code, message };
      }
    } catch (accessError) {
      return { kind: "access-error", reason: describeWasmFailure(accessError) };
    }
  }
  return { kind: "unstructured" };
}

export function parseWasmFailure(where: string, error: unknown): HostFailure {
  const classified = classifyWasmFailure(error);
  if (classified.kind === "structured") {
    return { code: classified.code, where, message: classified.message };
  }
  if (classified.kind === "access-error") {
    return { code: "WASM_PROTOCOL", where, message: `读取结构化 WASM 错误失败：${classified.reason}` };
  }
  return { code: "WASM_PROTOCOL", where, message: describeWasmFailure(error) };
}
