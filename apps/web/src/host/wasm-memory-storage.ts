import {
  checksumWasmMemoryRecord, parseWasmMemoryGenesis, parseWasmMemoryLedger, parseWasmMemorySnapshot,
  wasmMemoryGenesisKey, wasmMemoryLedgerKey, WASM_MEMORY_SCHEMA_VERSION,
} from "./wasm-memory-schema.ts";
import type { WasmMemoryLedgerEntry, WasmMemorySnapshot } from "./wasm-memory-schema.ts";
import { addWasmMemoryBytes, evaluateWasmMemoryBudget, rebuildWasmResidualBytes } from "./wasm-memory-budget.ts";

export interface WasmMemoryStoragePort {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}
export interface WasmMemoryStorageOptions {
  session: WasmMemoryStoragePort;
  ledger: WasmMemoryStoragePort;
  maximumBytes: number;
  reclaimBytes: number;
  measuredMaxLiveBytes: number | null;
  ledgerMaxBytes?: number;
  randomValues?: (bytes: Uint8Array) => void;
  now?: () => number;
}
export interface WasmMemoryStorageDiagnostic {
  code: string;
  where: string;
  message: string;
  cause?: unknown;
}
export interface WasmMemoryStorageRecovery {
  kind: "restored" | "not-ready";
  chainId: string;
  carriedResidualBytes: number | null;
  reloadCount: number;
  source: "new-chain" | "snapshot" | "ledger" | "genesis";
  diagnostics: readonly WasmMemoryStorageDiagnostic[];
}
export interface WasmMemorySnapshotAnchor {
  carryBase: number;
  sessionResident: number;
  liveUpperBound: number;
  reloadCount: number;
}
export class WasmMemoryStorageError extends Error {
  code: string;
  where: string;
  context: Readonly<Record<string, unknown>>;
  recoveryActions: readonly string[];
  constructor(code: string, where: string, message: string, context: Readonly<Record<string, unknown>>, cause?: unknown) {
    super(message, { cause });
    this.code = code; this.where = where; this.context = context;
    this.recoveryActions = ["保留错误详情反馈", "关闭标签页后重新打开"];
  }
}

const CHAIN_KEY = "wasm-chain";
const SNAPSHOT_KEY = "wasm-residual-state";
const DEFAULT_LEDGER_MAX_BYTES = 64 * 1024;
const READ_ROUNDS = 3;

type LedgerView = { carry: number; reloadCount: number; hasGenesis: boolean; hasEntries: boolean };

function seal<T extends Record<string, unknown>>(body: T) {
  return { ...body, checksum: checksumWasmMemoryRecord(body) };
}
function validInteger(value: unknown, name: string): number {
  if (typeof value !== "number" || !Number.isSafeInteger(value) || value < 0) throw new Error(`${name} 必须为非负 safe integer`);
  return value;
}

/** 文档级同步残留事实存储；本类不执行 reload、预算准入、回收或实例生命周期。 */
export class WasmMemoryStorage {
  readonly forkId: string;
  private readonly options: WasmMemoryStorageOptions;
  private readonly ledgerMaxBytes: number;
  private readonly notices: WasmMemoryStorageDiagnostic[] = [];
  private attempted = false;
  private opened = false;
  private chainId: string | null = null;
  private gate: WasmMemoryStorageError | null = null;
  private dirty = false;
  private baseCarry: number | null = null;
  private resident = 0;
  private lastWrittenSeq: number | null = null;
  private lastWrittenCarry = 0;

  constructor(options: WasmMemoryStorageOptions) {
    this.options = options;
    evaluateWasmMemoryBudget({ maximumBytes: options.maximumBytes, reclaimBytes: options.reclaimBytes,
      measuredMaxLiveBytes: options.measuredMaxLiveBytes, carryBase: 0, sessionResident: 0, liveBytes: 0 }, "admission");
    this.ledgerMaxBytes = options.ledgerMaxBytes === undefined ? DEFAULT_LEDGER_MAX_BYTES : validInteger(options.ledgerMaxBytes, "ledgerMaxBytes");
    if (this.ledgerMaxBytes === 0) throw new Error("ledgerMaxBytes 必须为正 bytes");
    this.forkId = this.randomId();
  }

  private error(code: string, message: string, context: Record<string, unknown>, cause?: unknown): WasmMemoryStorageError {
    return new WasmMemoryStorageError(code, "wasm-memory-storage", message, context, cause);
  }
  private corrupt(reason: string, cause?: unknown): WasmMemoryStorageError {
    return this.error("MEMORY_HANDOVER_CORRUPT", `残留账无法安全读回：${reason}；请保留详情反馈，不能清账继续`, { reason }, cause);
  }
  private note(code: string, message: string, cause?: unknown): void {
    this.notices.push({ code, where: "wasm-memory-storage", message, ...(cause === undefined ? {} : { cause }) });
  }
  private randomId(): string {
    const bytes = new Uint8Array(16);
    try {
      if (this.options.randomValues === undefined) globalThis.crypto.getRandomValues(bytes);
      else this.options.randomValues(bytes);
    } catch (cause) { throw this.error("MEMORY_CHAIN_UNAVAILABLE", "无法生成128bit残留链/文档写者身份", { channel: "crypto" }, cause); }
    return [...bytes].map((byte) => byte.toString(16).padStart(2, "0")).join("");
  }
  private now(): number { return validInteger(this.options.now === undefined ? Date.now() : this.options.now(), "ts（毫秒）"); }
  private context() {
    if (this.chainId === null) throw new Error("必须先 open 初始化残留链");
    return { chainId: this.chainId, maximumBytes: this.options.maximumBytes };
  }
  private write(port: WasmMemoryStoragePort, key: string, encoded: string): void {
    const failures: unknown[] = [];
    for (let attempt = 0; attempt < 2; attempt += 1) {
      try {
        port.setItem(key, encoded);
        if (failures.length > 0) this.note("MEMORY_STORAGE_WRITE_RETRIED", `残留存储写入重试成功：${key}；首次错误保留供反馈`, failures[0]);
        return;
      }
      catch (cause) { failures.push(cause); }
    }
    throw new AggregateError(failures, `残留存储写入两次失败：${key}`);
  }
  private requireOpen(): void {
    if (this.gate !== null) throw this.gate;
    if (!this.opened) throw new Error("必须先 open 初始化残留链");
  }
  diagnostics(): readonly WasmMemoryStorageDiagnostic[] { return [...this.notices]; }
  assertAdmissionWritable(): void { this.requireOpen(); }

  open(): WasmMemoryStorageRecovery {
    if (this.attempted) throw new Error("每文档 owner 只允许 open 一次，禁止重复继承");
    this.attempted = true;
    let chain: string | null;
    try { chain = this.options.session.getItem(CHAIN_KEY); }
    catch (cause) { throw this.error("MEMORY_CHAIN_UNAVAILABLE", "无法读取残留链身份，请检查sessionStorage并反馈", { channel: "session" }, cause); }
    if (chain === null) return this.createChain();
    try { wasmMemoryGenesisKey(chain); }
    catch (cause) { throw this.corrupt("CHAIN_ID_INVALID", cause); }
    this.chainId = chain;
    let snapshot: WasmMemorySnapshot | null = null;
    try {
      const encoded = this.options.session.getItem(SNAPSHOT_KEY);
      if (encoded === null) this.note("MEMORY_SNAPSHOT_MISSING", "本链残留快照丢失，必须依赖可读ledger锚保守重建");
      else if (!this.dirty) snapshot = parseWasmMemorySnapshot(JSON.parse(encoded), this.context());
    } catch (cause) { this.note("MEMORY_SNAPSHOT_UNREADABLE", "残留快照无效，尝试已锚定台账重建", cause); }
    try {
      if (this.options.ledger.getItem(wasmMemoryLedgerKey(chain, this.forkId)) !== null) {
        throw this.error("MEMORY_HANDOVER_CORRUPT", "文档fork写者身份碰撞，禁止覆盖已有台账", { reason: "FORK_ID_COLLISION" });
      }
    } catch (cause) {
      if (cause instanceof WasmMemoryStorageError) throw cause;
      // 副通道读取失败不能前置推翻绿快照；正推进仍须后续checkpoint写入成功。
      this.note("MEMORY_LEDGER_READ_FAILED", "无法读取本人fork键以核对写者身份；不把未知读值当作空键", cause);
    }
    let ledger: LedgerView | null = null;
    let ledgerFailure: unknown;
    try {
      const observed = this.readLedger(); ledger = observed.view; ledgerFailure = observed.failure;
    } catch (cause) { ledgerFailure = cause; }
    if (ledgerFailure !== undefined) {
      if (snapshot === null) throw ledgerFailure;
      // 主通道绿时副通道错误仅诊断；不能把失败读伪装成可靠零账。
      this.note("MEMORY_LEDGER_READ_FAILED", "主快照可读，但台账存在读错误；已验证高账仍须先核stale", ledgerFailure);
    }
    let source: WasmMemoryStorageRecovery["source"];
    let carry: number | null;
    let reloadCount: number;
    const snapshotTotal = snapshot === null ? null : addWasmMemoryBytes(snapshot.carryBase, snapshot.sessionResident, snapshot.liveUpperBound);
    if (snapshot !== null && snapshotTotal !== null && (ledger === null || ledger.carry <= snapshotTotal)) {
      if (ledger !== null && !ledger.hasGenesis && !ledger.hasEntries) {
        this.note("MEMORY_LEDGER_ANCHOR_MISSING", "主快照有效，但本链ledger无可读锚；缺失已登记，不清账重置");
      }
      source = "snapshot"; carry = snapshotTotal; reloadCount = snapshot.reloadCount;
    } else {
      if (snapshot !== null) this.note("MEMORY_SNAPSHOT_STALE", "快照三和落后台账highWater，按保守上界重建");
      // 可信高账证明主通道过期后，必须严格重建；不能拿错误扫描的部分并集恢复。
      if (ledgerFailure !== undefined) throw ledgerFailure;
      if (ledger === null) throw this.corrupt("NO_VALID_ANCHOR");
      if (!ledger.hasEntries && !ledger.hasGenesis) throw this.corrupt("NO_VALID_ANCHOR");
      source = ledger.hasEntries ? "ledger" : "genesis";
      carry = ledger.hasEntries ? rebuildWasmResidualBytes(ledger.carry, this.options) : this.options.maximumBytes;
      reloadCount = ledger.reloadCount;
      if (carry !== null) this.note("MEMORY_RESIDUAL_REBUILT", ledger.hasEntries
        ? "快照不可用或已过期，按highWater加双上界补足并封顶继承；可能额外保守多记"
        : "快照丢失且仅genesis锚可读，按maximum最坏上界继承，后续开局仍须预算预检");
    }
    if (carry === null) {
      this.gate = this.error("MEMORY_MEASUREMENT_NOT_READY", "缺少真实全局maxLive标定，残留重建未就绪", { channel: "measurement" });
      return { kind: "not-ready", chainId: chain, carriedResidualBytes: null, reloadCount, source, diagnostics: this.diagnostics() };
    }
    this.opened = true;
    this.baseCarry = carry;
    if (carry > 0 && (ledger === null || carry > ledger.carry)) {
      try { this.append("f5", carry, reloadCount); }
      catch (cause) {
        this.gate = this.error("MEMORY_HANDOVER_WRITE_FAILED", "恢复正推进checkpoint写入失败，禁止开局", { channel: "ledger", context: "recovery" }, cause);
        throw this.gate;
      }
    }
    this.writeSnapshot({ carryBase: carry, sessionResident: 0, liveUpperBound: 0, reloadCount });
    return { kind: "restored", chainId: chain, carriedResidualBytes: carry, reloadCount, source, diagnostics: this.diagnostics() };
  }

  private createChain(): WasmMemoryStorageRecovery {
    const chainId = this.randomId(); this.chainId = chainId;
    const genesis = seal({ schemaVersion: WASM_MEMORY_SCHEMA_VERSION, seq: 0, chainId,
      carriedResidualBytes: 0, reloadCount: 0, handoverNonce: this.randomId(), outcome: "genesis", ts: this.now(), highWater: { carry: 0, reloadCount: 0 } });
    const snapshot = seal({ schemaVersion: WASM_MEMORY_SCHEMA_VERSION, chainId, carryBase: 0,
      sessionResident: 0, liveUpperBound: 0, reloadCount: 0, genesis: true });
    try {
      // 先完整双锚，最后发布chain；失败半成品不能在下次加载冒充既存链。
      this.write(this.options.ledger, wasmMemoryGenesisKey(chainId), JSON.stringify(genesis));
      this.write(this.options.session, SNAPSHOT_KEY, JSON.stringify(snapshot));
      this.write(this.options.session, CHAIN_KEY, chainId);
    } catch (cause) {
      this.gate = this.error("MEMORY_CHAIN_UNAVAILABLE", "残留链双锚无法建立，禁止开局", { channel: "genesis" }, cause);
      throw this.gate;
    }
    this.opened = true;
    this.baseCarry = 0;
    return { kind: "restored", chainId, carriedResidualBytes: 0, reloadCount: 0, source: "new-chain", diagnostics: this.diagnostics() };
  }

  private readLedger(): { view: LedgerView; failure?: unknown } {
    const raw = this.scanKeys();
    const result: LedgerView = { carry: 0, reloadCount: 0, hasGenesis: false, hasEntries: false };
    let failure: unknown;
    for (const [key, encoded] of raw) {
      if (key === wasmMemoryGenesisKey(this.context().chainId)) {
        try { parseWasmMemoryGenesis(JSON.parse(encoded), this.context(), key); result.hasGenesis = true; }
        catch (cause) { this.note("MEMORY_LEDGER_ENTRY_DISCARDED", `genesis不可读：${key}`, cause); }
        continue;
      }
      let rows: WasmMemoryLedgerEntry[];
      try { rows = this.readRows(key, encoded, true); }
      catch (cause) {
        // 完整无骑跨扫描内，别键错误不能抹去独立绿锚的stale证明。
        // 保留错误只供后续严格拒绝；绝不据部分并集重建。
        if (failure === undefined) failure = cause;
        continue;
      }
      for (const row of rows) {
        result.hasEntries = true; result.carry = Math.max(result.carry, row.highWater.carry);
        result.reloadCount = Math.max(result.reloadCount, row.highWater.reloadCount);
      }
    }
    return { view: result, ...(failure === undefined ? {} : { failure }) };
  }

  private scanKeys(): Map<string, string> {
    const { chainId } = this.context();
    const prefix = `wasm-residual-ledger:${chainId}:`;
    const genesisKey = wasmMemoryGenesisKey(chainId);
    const keys = () => Object.keys(this.options.ledger).filter((key) => key === genesisKey || key.startsWith(prefix) || key === `wasm-residual-ledger:${chainId}`);
    for (let outer = 0; outer < READ_ROUNDS; outer += 1) {
      const values = new Map<string, string>();
      let current: string[];
      try { current = keys(); }
      catch (cause) { throw this.corrupt("ENUMERATION_FAILED", cause); }
      let straddled = false;
      for (let round = 0; round < READ_ROUNDS; round += 1) {
        for (const key of current) {
          if (values.has(key)) continue;
          let value: string | null;
          try { value = this.options.ledger.getItem(key); }
          catch (cause) { throw this.corrupt("LEDGER_READ_FAILED", cause); }
          if (value === null) { straddled = true; break; }
          values.set(key, value);
        }
        if (straddled) break;
        let next: string[];
        try { next = keys(); }
        catch (cause) { throw this.corrupt("ENUMERATION_FAILED", cause); }
        const additions = next.filter((key) => !values.has(key));
        if (additions.length === 0) return values;
        current = additions;
      }
      if (!straddled) throw this.corrupt("ENUMERATION_UNSTABLE");
      this.note("MEMORY_LEDGER_STRADDLE_RETRY", "已枚举键被并发删除，弃置本轮全部缓存后重枚举");
    }
    throw this.corrupt("STRADDLE_LIMIT");
  }

  private readRows(key: string, encoded: string, discardBad: boolean): WasmMemoryLedgerEntry[] {
    const context = this.context();
    const forkId = key.slice(`wasm-residual-ledger:${context.chainId}:`.length);
    const fail = (cause: unknown): WasmMemoryLedgerEntry[] => {
      if (!discardBad) throw cause;
      this.note("MEMORY_LEDGER_ENTRY_DISCARDED", `台账记录不可读：${key}`, cause); return [];
    };
    let raw: unknown;
    try {
      if (wasmMemoryLedgerKey(context.chainId, forkId) !== key) throw new Error("ledger key不匹配");
      raw = JSON.parse(encoded);
      if (!Array.isArray(raw) || raw.length === 0) throw new Error("ledger必须为非空数组");
    } catch (cause) { return fail(cause); }
    const result: WasmMemoryLedgerEntry[] = [];
    let previousIndex = -1;
    for (let index = 0; index < raw.length; index += 1) {
      let row: WasmMemoryLedgerEntry;
      try { row = parseWasmMemoryLedger([raw[index]], { ...context, forkId }, key)[0]; }
      catch (cause) { if (!discardBad) throw cause; this.note("MEMORY_LEDGER_ENTRY_DISCARDED", `台账单条校验失败：${key}[${index}]`, cause); continue; }
      const previous = result.at(-1);
      if (previous !== undefined) {
        if (row.seq <= previous.seq) throw this.corrupt("SEQ_REGRESSED");
        if (row.highWater.carry < previous.highWater.carry || (index === previousIndex + 1 && row.highWater.carry !== Math.max(previous.highWater.carry, row.carriedResidualBytes))) {
          const cause = new Error("highWater.carry 未按可验证的单键历史承接");
          if (!discardBad) throw cause;
          this.note("MEMORY_LEDGER_ENTRY_DISCARDED", `台账highWater单条自矛盾：${key}[${index}]`, cause);
          continue;
        }
      }
      result.push(row); previousIndex = index;
    }
    return result;
  }

  append(outcome: WasmMemoryLedgerEntry["outcome"], carriedResidualBytes: number, reloadCount: number): void {
    this.requireOpen();
    const context = this.context(); const key = wasmMemoryLedgerKey(context.chainId, this.forkId);
    try {
      const encoded = this.options.ledger.getItem(key);
      const previous = encoded === null ? [] : this.readRows(key, encoded, false);
      const tail = previous.at(-1);
      if ((this.lastWrittenSeq === null && tail !== undefined) ||
          (this.lastWrittenSeq !== null && (tail === undefined || tail.seq !== this.lastWrittenSeq || tail.highWater.carry < this.lastWrittenCarry))) {
        throw this.corrupt("OWN_KEY_HISTORY_CHANGED");
      }
      const seq = tail === undefined ? 0 : addWasmMemoryBytes(tail.seq, 1);
      const row = seal({ schemaVersion: WASM_MEMORY_SCHEMA_VERSION, seq, chainId: context.chainId,
        carriedResidualBytes, reloadCount, handoverNonce: this.randomId(), outcome, ts: this.now(),
        highWater: { carry: Math.max(tail === undefined ? 0 : tail.highWater.carry, carriedResidualBytes), reloadCount: Math.max(tail === undefined ? 0 : tail.highWater.reloadCount, reloadCount) } });
      parseWasmMemoryLedger([row], { ...context, forkId: this.forkId }, key);
      const rows = [...previous, row];
      let text = JSON.stringify(rows);
      while (new TextEncoder().encode(text).byteLength > this.ledgerMaxBytes && rows.length > 1) { rows.shift(); text = JSON.stringify(rows); }
      if (new TextEncoder().encode(text).byteLength > this.ledgerMaxBytes) throw new Error("单条ledger超过per-key字节上限，不能丢弃最后highWater");
      this.write(this.options.ledger, key, text);
      this.lastWrittenSeq = seq; this.lastWrittenCarry = row.highWater.carry;
    } catch (cause) { throw this.error("MEMORY_HANDOVER_WRITE_FAILED", "台账写入失败，请保留原账并反馈", { channel: "ledger", context: "append", key }, cause); }
  }

  writeSnapshot(anchor: WasmMemorySnapshotAnchor): void {
    this.requireOpen();
    const context = this.context();
    const fields = ["carryBase", "sessionResident", "liveUpperBound", "reloadCount"];
    if (anchor === null || typeof anchor !== "object" || Array.isArray(anchor) ||
        Reflect.ownKeys(anchor).some((key) => typeof key !== "string" || !fields.includes(key)) ||
        fields.some((key) => !Object.hasOwn(anchor, key))) throw new Error("snapshot anchor 含未知字段或缺少字段");
    const candidate = seal({ ...anchor, schemaVersion: WASM_MEMORY_SCHEMA_VERSION, chainId: context.chainId, genesis: false });
    const parsed = parseWasmMemorySnapshot(candidate, context);
    if (parsed.carryBase !== this.baseCarry) throw new Error("carryBase 必须保持本次open继承值");
    if (parsed.sessionResident < this.resident) throw new Error("sessionResident 不得回退，drop终值累计只能增加");
    const total = addWasmMemoryBytes(parsed.carryBase, parsed.sessionResident, parsed.liveUpperBound);
    // 合法目标被接受后才推进内存账；持久化失败不能把事实退回旧锚。
    this.resident = parsed.sessionResident;
    try { this.write(this.options.session, SNAPSHOT_KEY, JSON.stringify(candidate)); this.dirty = false; }
    catch (cause) {
      this.dirty = true;
      this.note("MEMORY_SNAPSHOT_WRITE_FAILED", "快照锚写入两败，已标记dirty并尝试台账备份", cause);
      try { this.append("snapshot-write-fail", total, parsed.reloadCount); }
      catch (ledgerCause) {
        this.gate = this.error("MEMORY_HANDOVER_WRITE_FAILED", "快照与台账双通道写入失败，禁止开局", { channel: "both", context: "anchor" }, new AggregateError([cause, ledgerCause], "双通道写入失败"));
        throw this.gate;
      }
    }
  }
}
