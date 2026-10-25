export type WasmFailureClassification =
  | { readonly kind: "structured"; readonly code: string; readonly message: string }
  | { readonly kind: "access-error"; readonly field: string; readonly reason: string; readonly cause: Readonly<Record<string, unknown>> }
  | { readonly kind: "unstructured"; readonly description: string; readonly cause?: Readonly<Record<string, unknown>> };

function safeString(value: unknown): string {
  try {
    return String(value);
  } catch {
    return "<无法读取错误详情>";
  }
}

export function describeWasmFailure(error: unknown, coreFields?: { readonly code: unknown; readonly message: unknown }): string {
  if (typeof error === "string") return error;
  try {
    if (error instanceof Error) return safeString(coreFields === undefined ? error.message : coreFields.message);
  } catch (inspectionError) {
    return `检查错误对象失败：${safeString(inspectionError)}`;
  }
  if (error !== null && typeof error === "object") {
    try {
      // 分类已读的核心字段必须复用；序列化诊断不得再次触发原始 getter。
      let source: unknown = error;
      if (coreFields !== undefined) {
        const prototype = Object.getPrototypeOf(error);
        // 标准原生标量先取得不可变值，不让序列化重新访问原对象的核心字段。
        const ownToJSON = Object.hasOwn(error, "toJSON");
        if (!ownToJSON && prototype === Date.prototype) {
          source = Number.isNaN(Date.prototype.getTime.call(error)) ? null : Date.prototype.toISOString.call(error);
        } else if (!ownToJSON && prototype === URL.prototype) {
          source = URL.prototype.toString.call(error);
        } else if (!ownToJSON && prototype === Number.prototype) source = Number.prototype.valueOf.call(error);
        else if (!ownToJSON && prototype === Boolean.prototype) source = Boolean.prototype.valueOf.call(error);
        else if (!ownToJSON && prototype === String.prototype) source = String.prototype.valueOf.call(error);
        else {
          const descriptors = Object.getOwnPropertyDescriptors(error);
          for (const key of ["code", "message"] as const) {
            descriptors[key] = { value: coreFields[key], enumerable: descriptors[key]?.enumerable === true, configurable: true };
          }
          // descriptor 也可能来自 Proxy，必须始终投影；custom toJSON 只接触缓存字段。
          source = Object.create(prototype, descriptors);
        }
      }
      return `非标准错误对象：${JSON.stringify(source)}`;
    } catch (serializationError) {
      let reason: string;
      try {
        reason = serializationError instanceof Error ? safeString(serializationError.message) : safeString(serializationError);
      } catch (inspectionError) {
        reason = `检查序列化错误失败：${safeString(inspectionError)}`;
      }
      const original = coreFields === undefined ? "" : Object.entries(coreFields)
        .filter(([, value]) => typeof value === "string")
        .map(([key, value]) => `${key}=${String(value)}`).join("；");
      return `非标准错误对象无法序列化：${reason}${original.length > 0 ? `；已读原始错误：${original}` : ""}`;
    }
  }
  return safeString(error);
}

export function classifyWasmFailure(error: unknown): WasmFailureClassification {
  if (error !== null && typeof error === "object") {
    let field = "object";
    const cause: Record<string, unknown> = {};
    try {
      if (Array.isArray(error)) return { kind: "unstructured", description: describeWasmFailure(error) };
      const candidate = error as Readonly<Record<string, unknown>>;
      field = "code";
      const code = candidate.code;
      if (code === null || ["string", "number", "boolean", "bigint"].includes(typeof code)) cause.code = code;
      field = "message";
      const message = candidate.message;
      if (message === null || ["string", "number", "boolean", "bigint"].includes(typeof message)) cause.message = message;
      if (
        typeof code === "string"
        && code.length > 0
        && typeof message === "string"
        && message.length > 0
      ) {
        return { kind: "structured", code, message };
      }
      return { kind: "unstructured", description: describeWasmFailure(error, { code, message }), cause };
    } catch (accessError) {
      return { kind: "access-error", field, reason: describeWasmFailure(accessError), cause };
    }
  }
  return { kind: "unstructured", description: describeWasmFailure(error) };
}
