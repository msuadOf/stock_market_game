import { ProtocolError } from "./types.ts";

export type ProtocolRecord = Readonly<Record<string, unknown>>;

function malformed(path: string, message: string): never {
  throw new ProtocolError("PROTOCOL_MALFORMED", path, message);
}

export function isRecord(value: unknown): value is ProtocolRecord {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function record(value: unknown, path: string): ProtocolRecord {
  if (!isRecord(value)) malformed(path, `${path} 必须是对象`);
  return value;
}

export function exact(value: ProtocolRecord, keys: readonly string[], path: string): void {
  const actual = Object.keys(value);
  if (actual.length !== keys.length || keys.some((key) => !Object.hasOwn(value, key))) {
    malformed(path, `${path} 字段不符合协议契约`);
  }
}

export function field(value: ProtocolRecord, key: string, path: string): unknown {
  if (!Object.hasOwn(value, key)) malformed(path, `${path}.${key} 缺失`);
  return value[key];
}

export function text(value: unknown, path: string): string {
  if (typeof value !== "string") malformed(path, `${path} 必须是字符串`);
  return value;
}

export function enumValue<const T extends string>(value: unknown, allowed: readonly T[], path: string): T {
  const parsed = text(value, path);
  const matched = allowed.find((candidate) => candidate === parsed);
  if (matched === undefined) malformed(path, `${path} 是未知变体：${parsed}`);
  return matched;
}

export function boolean(value: unknown, path: string): boolean {
  if (typeof value !== "boolean") malformed(path, `${path} 必须是布尔值`);
  return value;
}

export function safeInteger(value: unknown, path: string, minimum = 0): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < minimum) {
    malformed(path, `${path} 必须是不小于 ${minimum} 的安全整数`);
  }
  return value;
}
export function safeU32(value: unknown, path: string): number {
  const parsed = safeInteger(value, path);
  if (parsed > 4_294_967_295) malformed(path, `${path} 必须在 u32 范围内`);
  return parsed;
}

export function safeU8(value: unknown, path: string): number {
  const parsed = safeInteger(value, path);
  if (parsed > 255) malformed(path, `${path} 必须在 u8 范围内`);
  return parsed;
}

export function finiteNumber(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isFinite(value)) {
    malformed(path, `${path} 必须是有限数字`);
  }
  return value;
}

export function signedSafeInteger(value: unknown, path: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value)) {
    malformed(path, `${path} 必须是安全整数`);
  }
  return value;
}

export function values(value: unknown, path: string): readonly unknown[] {
  if (!Array.isArray(value)) malformed(path, `${path} 必须是数组`);
  return value;
}

export function nullable<T>(
  value: unknown,
  parse: (entry: unknown, path: string) => T,
  path: string,
): T | null {
  return value === null ? null : parse(value, path);
}

export function mapEntries<T>(
  value: unknown,
  path: string,
  parse: (entry: unknown, entryPath: string) => T,
): Record<string, T> {
  const source = record(value, path);
  const parsed: Record<string, T> = Object.create(null);
  for (const [key, entry] of Object.entries(source)) {
    const normalizedKey = normalizeObjectKey(key, path);
    parsed[normalizedKey] = parse(entry, `${path}.${normalizedKey}`);
  }
  return parsed;
}

function rawMapKey(value: unknown, path: string): { readonly original: string; readonly normalized: string } {
  if (typeof value === "string") return { original: `string:${value}`, normalized: validateStructuralKey(value, path) };
  if (typeof value === "number" && Number.isSafeInteger(value)) {
    return { original: `number:${value}`, normalized: validateStructuralKey(String(value), path) };
  }
  if (typeof value === "bigint") {
    const normalized = Number(value);
    if (Number.isSafeInteger(normalized)) {
      return { original: `bigint:${value}`, normalized: validateStructuralKey(String(normalized), path) };
    }
  }
  return malformed(path, `${path} 包含非法 Map 键`);
}

function validateStructuralKey(key: string, path: string): string {
  if (key.length === 0 || key === "__proto__" || key === "prototype" || key === "constructor") {
    malformed(path, `${path} 包含非法 Map 键`);
  }
  return key;
}

function normalizeObjectKey(key: string, path: string): string {
  return validateStructuralKey(key, path);
}

function normalizeMap(value: ReadonlyMap<unknown, unknown>, path: string): ProtocolRecord {
  const normalized: Record<string, unknown> = Object.create(null);
  const rawKeys = new Map<string, string>();
  for (const [key, entry] of value) {
    const mapKey = rawMapKey(key, path);
    const previousRaw = rawKeys.get(mapKey.normalized);
    if (previousRaw !== undefined) {
      malformed(path, `${path} 包含重复 Map 键`);
    }
    rawKeys.set(mapKey.normalized, mapKey.original);
    normalized[mapKey.normalized] = normalizeSerdeValue(entry, `${path}.${mapKey.normalized}`);
  }
  return normalized;
}

export function normalizeSerdeValue(value: unknown, path = "protocol.normalize"): unknown {
  if (typeof value === "bigint") {
    const normalized = Number(value);
    if (!Number.isSafeInteger(normalized)) {
      malformed(path, `WASM 整数超出安全范围：${value}`);
    }
    return normalized;
  }
  if (value instanceof Map) return normalizeMap(value, path);
  if (Array.isArray(value)) return value.map((entry, index) => normalizeSerdeValue(entry, `${path}[${index}]`));
  if (isRecord(value)) {
    const normalized: Record<string, unknown> = Object.create(null);
    const keys = new Set<string>();
    for (const [key, entry] of Object.entries(value)) {
      const normalizedKey = normalizeObjectKey(key, path);
      if (keys.has(normalizedKey)) malformed(path, `${path} 包含重复对象键`);
      keys.add(normalizedKey);
      normalized[normalizedKey] = normalizeSerdeValue(entry, `${path}.${normalizedKey}`);
    }
    return normalized;
  }
  return value;
}
