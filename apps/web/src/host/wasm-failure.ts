export type WasmFailureClassification =
  | { readonly kind: "structured"; readonly code: string; readonly message: string }
  | { readonly kind: "access-error"; readonly reason: string }
  // 保留首读核心字段生成的描述，避免错误报告再次触发 getter。
  | { readonly kind: "unstructured"; readonly description: string; readonly code?: string };

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

function diagnosticSnapshot(error: object, code: unknown, message: unknown): object {
  const snapshot = Object.create(null) as Record<string, unknown>;
  const keys = Object.keys(error);
  for (const key of new Set([...keys, "toJSON"])) {
    Object.defineProperty(snapshot, key, {
      enumerable: keys.includes(key),
      get() {
        if (key === "code") return code;
        if (key === "message") return message;
        const value: unknown = Reflect.get(error, key, error);
        // 私有字段需要原对象的 brand，getter 与 toJSON 都保留原 receiver。
        if (key === "toJSON" && typeof value === "function") return (...args: unknown[]) => Reflect.apply(value, error, args);
        return value;
      },
    });
  }
  return snapshot;
}

export function classifyWasmFailure(error: unknown): WasmFailureClassification {
  if (error !== null && typeof error === "object") {
    try {
      if (Array.isArray(error)) return { kind: "unstructured", description: describeWasmFailure(error) };
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
      // JSON.stringify 也会读取可枚举的核心字段；只让它看到首读快照。
      const description = error instanceof Error ? safeString(message) : describeWasmFailure(diagnosticSnapshot(error, code, message));
      return { kind: "unstructured", description, ...(typeof code === "string" && code.length > 0 ? { code } : {}) };
    } catch (accessError) {
      return { kind: "access-error", reason: describeWasmFailure(accessError) };
    }
  }
  return { kind: "unstructured", description: describeWasmFailure(error) };
}
