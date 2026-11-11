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

export function describeReadFailureFields(fields: Readonly<Record<string, unknown>> | undefined): string {
  if (fields === undefined) return "";
  const original = Object.entries(fields)
    .filter(([, value]) => value === null || ["string", "number", "boolean", "bigint"].includes(typeof value))
    .map(([key, value]) => `${key}=${String(value)}`).join("；");
  return original.length > 0 ? `；已读原始错误：${original}` : "";
}

export function describeWasmFailure(error: unknown, coreFields?: { readonly code: unknown; readonly message: unknown; readonly where?: unknown }): string {
  if (typeof error === "string") return error;
  try {
    if (error instanceof Error) return safeString(coreFields === undefined ? error.message : coreFields.message);
  } catch (inspectionError) {
    return `检查错误对象失败：${safeString(inspectionError)}${describeReadFailureFields(coreFields)}`;
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
          for (const key of ["code", "message", "where"] as const) {
            if (!Object.hasOwn(coreFields, key)) continue;
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
      return `非标准错误对象无法序列化：${reason}${describeReadFailureFields(coreFields)}`;
    }
  }
  return safeString(error);
}

/** structuredClone 会删除 Error 的自定义字段与 AggregateError.errors，传输前保留这些诊断。 */
export function cloneWasmFailureDetails(value: unknown): unknown {
  const seen = new Map<object, unknown>();
  function project(source: unknown): unknown {
    if (source === null || typeof source !== "object") return source;
    if (seen.has(source)) return seen.get(source);
    if (source instanceof Date || source instanceof RegExp || source instanceof ArrayBuffer || (typeof SharedArrayBuffer !== "undefined" && source instanceof SharedArrayBuffer) || ArrayBuffer.isView(source)) return source;
    if (source instanceof Map) {
      const result = new Map(); seen.set(source, result);
      for (const [key, entry] of source) result.set(project(key), project(entry));
      return result;
    }
    if (source instanceof Set) {
      const result = new Set(); seen.set(source, result);
      for (const entry of source) result.add(project(entry));
      return result;
    }
    const nativeError = source instanceof Error;
    const prototype = Object.getPrototypeOf(source);
    const constructor = prototype === null ? undefined : Object.getOwnPropertyDescriptor(prototype, "constructor");
    // 未知原生对象仍由 structuredClone 决定可传输性，不能把 Promise/WeakMap 等变成空对象。
    if (!nativeError && !Array.isArray(source) && prototype !== null && prototype !== Object.prototype
      && typeof constructor?.value === "function" && Function.prototype.toString.call(constructor.value).includes("[native code]")) return source;
    const result: Record<string, unknown> | unknown[] = Array.isArray(source) ? new Array(source.length) : {};
    seen.set(source, result);
    const descriptors = Object.getOwnPropertyDescriptors(source);
    if (nativeError) {
      const stack = wasmFailureStack(source);
      if (stack !== undefined) descriptors.stack = { value: stack, enumerable: true };
      if (!Object.hasOwn(descriptors, "name")) {
        let prototype = Object.getPrototypeOf(source);
        while (prototype !== null) {
          const name = Object.getOwnPropertyDescriptor(prototype, "name");
          if (name !== undefined) { descriptors.name = name; break; }
          prototype = Object.getPrototypeOf(prototype);
        }
      }
    }
    for (const [key, descriptor] of Object.entries(descriptors)) {
      if (!nativeError && !descriptor.enumerable) continue;
      const detail = Object.hasOwn(descriptor, "value") ? descriptor.value
        : nativeError ? "[详情读取失败：访问器字段未执行]" : Reflect.get(source, key);
      Object.defineProperty(result, key, { value: project(detail), enumerable: true, configurable: true, writable: true });
    }
    return result;
  }
  return structuredClone(project(value));
}

/** 不重复执行核心 getter；V8 惰性 stack 仅在 message 为数据字段时读取。 */
export function wasmFailureStack(error: unknown): string | undefined {
  if (error === null || typeof error !== "object") return undefined;
  const stack = Object.getOwnPropertyDescriptor(error, "stack");
  if (stack === undefined) return undefined;
  if (Object.hasOwn(stack, "value")) return typeof stack.value === "string" ? stack.value : undefined;
  function hasDataProperty(key: "message" | "name"): boolean {
    let target: object | null = error as object;
    while (target !== null) {
      const descriptor = Object.getOwnPropertyDescriptor(target, key);
      if (descriptor !== undefined) return Object.hasOwn(descriptor, "value");
      target = Object.getPrototypeOf(target);
    }
    return true;
  }
  if (error instanceof Error && stack.get !== undefined && Function.prototype.toString.call(stack.get).includes("[native code]")
    && hasDataProperty("message") && hasDataProperty("name")) {
    const value: unknown = stack.get.call(error);
    return typeof value === "string" ? value : undefined;
  }
  return undefined;
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
      field = "where";
      const coreFields: { code: unknown; message: unknown; where?: unknown } = { code, message };
      if (Object.hasOwn(candidate, "where")) {
        const where = candidate.where;
        coreFields.where = where;
        if (where === null || ["string", "number", "boolean", "bigint"].includes(typeof where)) cause.where = where;
      }
      const description = describeWasmFailure(error, coreFields);
      try {
        if (error instanceof Error) {
          const stack = wasmFailureStack(error);
          if (stack !== undefined) cause.stack = stack;
          for (const [key, descriptor] of Object.entries(Object.getOwnPropertyDescriptors(error))) {
            if (["code", "message", "where", "stack"].includes(key)) continue;
            Object.defineProperty(cause, key, { value: Object.hasOwn(descriptor, "value") ? descriptor.value : "[详情读取失败：访问器字段未执行]", enumerable: true });
          }
        }
      } catch (diagnosticError) {
        cause.diagnosticReadError = describeWasmFailure(diagnosticError);
      }
      const supplemental = Object.fromEntries(Object.entries(cause).filter(([key]) => ["code", "message", "where"].includes(key)));
      // 标准 Error 的说明就是原 message；其他形态统一附上已读 primitive 诊断。
      if (Object.hasOwn(cause, "message") && description === String(cause.message)) delete supplemental.message;
      return { kind: "unstructured", description: `${description}${describeReadFailureFields(supplemental)}`, cause };
    } catch (accessError) {
      return { kind: "access-error", field, reason: describeWasmFailure(accessError), cause };
    }
  }
  return { kind: "unstructured", description: describeWasmFailure(error) };
}
