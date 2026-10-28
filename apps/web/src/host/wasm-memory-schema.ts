import { addWasmMemoryBytes } from "./wasm-memory-budget.ts";

// 这是首次实际格式标识，不承诺历史格式迁移。
export const WASM_MEMORY_SCHEMA_VERSION = 1;

export interface WasmMemorySchemaContext {
  chainId: string;
  maximumBytes: number;
}
export interface WasmMemoryLedgerContext extends WasmMemorySchemaContext {
  forkId: string;
}
export interface WasmMemoryHighWater {
  carry: number;
  reloadCount: number;
}
export interface WasmMemoryLedgerEntry {
  schemaVersion: typeof WASM_MEMORY_SCHEMA_VERSION;
  seq: number;
  chainId: string;
  carriedResidualBytes: number;
  reloadCount: number;
  handoverNonce: string;
  outcome: "reload" | "exhausted" | "corrupt" | "f5" | "snapshot-write-fail";
  ts: number;
  highWater: WasmMemoryHighWater;
  checksum: string;
}
export interface WasmMemoryGenesis extends Omit<WasmMemoryLedgerEntry, "outcome"> {
  outcome: "genesis";
}
// 本叶只解析常驻基础快照；handover / pendingIntent 协议须由后续实现明确扩展。
export interface WasmMemorySnapshot {
  schemaVersion: typeof WASM_MEMORY_SCHEMA_VERSION;
  chainId: string;
  carryBase: number;
  sessionResident: number;
  liveUpperBound: number;
  reloadCount: number;
  genesis: boolean;
  checksum: string;
}

function object(value: unknown, field: string): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value) ||
      (Object.getPrototypeOf(value) !== Object.prototype && Object.getPrototypeOf(value) !== null)) {
    throw new Error(`${field} 必须为 JSON 对象`);
  }
  return value as Record<string, unknown>;
}
function exactFields(value: Record<string, unknown>, fields: readonly string[], where: string): void {
  for (const key of Reflect.ownKeys(value)) {
    if (typeof key !== "string" || !fields.includes(key)) throw new Error(`${where} 未知字段 ${String(key)}`);
  }
  for (const key of fields) {
    if (!Object.hasOwn(value, key)) throw new Error(`${where} 缺少字段 ${key}`);
  }
}
function integer(value: unknown, field: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) {
    throw new Error(`${field} 必须为非负 safe integer`);
  }
  return value;
}
function id128(value: unknown, field: string): string {
  if (typeof value !== "string" || !/^[0-9a-f]{32}$/.test(value)) throw new Error(`${field} 必须为 128bit 小写 hex id`);
  return value;
}
function validateContext(context: WasmMemorySchemaContext): void {
  id128(context.chainId, "chainId");
  if (integer(context.maximumBytes, "maximumBytes") === 0) throw new Error("maximumBytes 必须为正 bytes");
}
function boundedBytes(value: unknown, field: string, maximumBytes: number): number {
  const result = integer(value, field);
  if (result > maximumBytes) throw new Error(`${field} bytes 超过 maximumBytes`);
  return result;
}
function canonical(value: unknown): string {
  if (value === null || typeof value === "string" || typeof value === "boolean") return JSON.stringify(value);
  if (typeof value === "number" && Number.isFinite(value)) return JSON.stringify(value);
  if (Array.isArray(value)) {
    const values: string[] = [];
    for (let i = 0; i < value.length; i += 1) {
      if (!Object.hasOwn(value, i)) throw new Error("checksum 不接受稀疏数组");
      values.push(canonical(value[i]));
    }
    return `[${values.join(",")}]`;
  }
  const record = object(value, "checksum");
  if (Reflect.ownKeys(record).some((key) => typeof key !== "string")) throw new Error("checksum 不接受 Symbol 字段");
  return `{${Object.keys(record).sort().map((key) => `${JSON.stringify(key)}:${canonical(record[key])}`).join(",")}}`;
}

// 调用方传不含 checksum 的字段。FNV-1a 只检测意外损坏，不认证来源，也不证明未遭伪造。
export function checksumWasmMemoryRecord(body: unknown): string {
  let hash = 0x811c9dc5;
  for (const byte of new TextEncoder().encode(canonical(body))) hash = Math.imul(hash ^ byte, 0x01000193) >>> 0;
  return `fnv1a32:${hash.toString(16).padStart(8, "0")}`;
}
function checksum(record: Record<string, unknown>): string {
  if (typeof record.checksum !== "string" || !/^fnv1a32:[0-9a-f]{8}$/.test(record.checksum)) {
    throw new Error("checksum 必须为 fnv1a32 格式");
  }
  const { checksum: encoded, ...body } = record;
  if (checksumWasmMemoryRecord(body) !== encoded) throw new Error("checksum 校验失败：WASM memory 记录损坏");
  return encoded;
}
function identity(record: Record<string, unknown>, context: WasmMemorySchemaContext): void {
  if (record.schemaVersion !== WASM_MEMORY_SCHEMA_VERSION) throw new Error("schemaVersion 不受支持");
  if (id128(record.chainId, "chainId") !== context.chainId) throw new Error("chainId 与当前链不一致");
}

export function wasmMemoryLedgerKey(chainId: string, forkId: string): string {
  return `wasm-residual-ledger:${id128(chainId, "chainId")}:${id128(forkId, "forkId")}`;
}
export function wasmMemoryGenesisKey(chainId: string): string {
  return `wasm-residual-genesis:${id128(chainId, "chainId")}`;
}

const ledgerFields = ["schemaVersion", "seq", "chainId", "carriedResidualBytes", "reloadCount", "handoverNonce", "outcome", "ts", "highWater", "checksum"];
const outcomes = ["reload", "exhausted", "corrupt", "f5", "snapshot-write-fail"];
function parseEntry(value: unknown, context: WasmMemorySchemaContext, genesis: boolean): WasmMemoryLedgerEntry | WasmMemoryGenesis {
  const row = object(value, "ledger entry");
  exactFields(row, ledgerFields, "ledger entry");
  identity(row, context);
  const highWater = object(row.highWater, "highWater");
  exactFields(highWater, ["carry", "reloadCount"], "highWater");
  const carry = boundedBytes(highWater.carry, "highWater.carry", context.maximumBytes);
  const count = integer(highWater.reloadCount, "highWater.reloadCount");
  const carriedResidualBytes = boundedBytes(row.carriedResidualBytes, "carriedResidualBytes", context.maximumBytes);
  const reloadCount = integer(row.reloadCount, "reloadCount");
  const seq = integer(row.seq, "seq");
  if (genesis) {
    if (row.outcome !== "genesis" || seq !== 0 || carry !== 0 || carriedResidualBytes !== 0) {
      throw new Error("genesis 必须为 seq=0 的零锚");
    }
  } else if (typeof row.outcome !== "string" || !outcomes.includes(row.outcome)) {
    throw new Error("ledger outcome 不受支持，genesis 必须使用独立链级 key");
  }
  if (carry < carriedResidualBytes) throw new Error("highWater 未覆盖本条值");
  return {
    schemaVersion: WASM_MEMORY_SCHEMA_VERSION, seq, chainId: context.chainId, carriedResidualBytes,
    reloadCount, handoverNonce: id128(row.handoverNonce, "handoverNonce"),
    outcome: row.outcome as WasmMemoryLedgerEntry["outcome"] | "genesis",
    ts: integer(row.ts, "ts（毫秒）"), highWater: { carry, reloadCount: count }, checksum: checksum(row),
  };
}

// 这里只校验已经取得的单键值；不实现枚举追读、骑跨重试或孤儿回收。
export function parseWasmMemoryLedger(value: unknown, context: WasmMemoryLedgerContext, key: string): WasmMemoryLedgerEntry[] {
  validateContext(context);
  if (key !== wasmMemoryLedgerKey(context.chainId, context.forkId)) throw new Error("ledger key 与 chainId / forkId 不一致");
  if (!Array.isArray(value)) throw new Error("ledger 必须为数组");
  if (value.length === 0) throw new Error("ledger 不允许空数组；分叉 key 应惰性创建");
  const entries: WasmMemoryLedgerEntry[] = [];
  for (const item of value) {
    const entry = parseEntry(item, context, false) as WasmMemoryLedgerEntry;
    const previous = entries.at(-1);
    if (previous !== undefined) {
      if (entry.seq <= previous.seq) throw new Error("ledger 单键 seq 必须严格递增");
      if (entry.highWater.carry !== Math.max(previous.highWater.carry, entry.carriedResidualBytes)) {
        throw new Error("highWater.carry 未按单键历史最大值承接");
      }
      // reloadCount 仅作诊断，不用跨条目单调性改变可读性或准入。
    }
    entries.push(entry);
  }
  return entries;
}

export function parseWasmMemoryGenesis(value: unknown, context: WasmMemorySchemaContext, key: string): WasmMemoryGenesis {
  validateContext(context);
  if (key !== wasmMemoryGenesisKey(context.chainId)) throw new Error("genesis key 与 chainId 不一致");
  return parseEntry(value, context, true) as WasmMemoryGenesis;
}

export function parseWasmMemorySnapshot(value: unknown, context: WasmMemorySchemaContext): WasmMemorySnapshot {
  validateContext(context);
  const row = object(value, "snapshot");
  exactFields(row, ["schemaVersion", "chainId", "carryBase", "sessionResident", "liveUpperBound", "reloadCount", "genesis", "checksum"], "snapshot");
  identity(row, context);
  const carryBase = boundedBytes(row.carryBase, "carryBase", context.maximumBytes);
  const sessionResident = boundedBytes(row.sessionResident, "sessionResident", context.maximumBytes);
  const liveUpperBound = boundedBytes(row.liveUpperBound, "liveUpperBound", context.maximumBytes);
  const reloadCount = integer(row.reloadCount, "reloadCount");
  if (addWasmMemoryBytes(carryBase, sessionResident, liveUpperBound) > context.maximumBytes) throw new Error("snapshot 三和超过 maximumBytes");
  if (typeof row.genesis !== "boolean") throw new Error("snapshot.genesis 必须为 boolean");
  if (row.genesis && (carryBase !== 0 || sessionResident !== 0 || liveUpperBound !== 0)) throw new Error("snapshot genesis 必须为零锚");
  return { schemaVersion: WASM_MEMORY_SCHEMA_VERSION, chainId: context.chainId, carryBase, sessionResident, liveUpperBound, reloadCount, genesis: row.genesis, checksum: checksum(row) };
}
