import { saveMaterializationOrder } from "./materialization-order.ts"
import type { ArchiveContainerShape, ArchiveRecord, ArchiveRecordScanOptions, ArchiveRootScan } from "./archive-root-scan.ts"
import { exact, materializeSaveValue, RawSaveNumber, record, safeIntegerKey, SaveSchemaError } from "./primitives.ts"
import { parseSaveAccount, parseSaveSnapshot, SAVE_SNAPSHOT_KEYS } from "./save-snapshot.ts"
import { parseRuntimeStrategyState, parseSaveRuntime, SAVE_RUNTIME_KEYS } from "./runtime-state.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"

function shapeRecord(shape: ArchiveContainerShape): Record<string, unknown> {
  if (shape.kind === "number") return new RawSaveNumber("0") as unknown as Record<string, unknown>
  if (shape.kind !== "object") return record(null, String(shape.path[0]))
  return Object.fromEntries(shape.keys.map(key => [key, undefined]))
}

/** 只保留账户/策略计数和小型 metadata；完整跨域与日终检查仍由后续阶段完成。 */
export class SessionShardValidation {
  private accounts = 0
  private strategies = 0
  private shapes = new Map<string, ArchiveContainerShape>()
  readonly ledger: SchemaFailureLedger
  constructor(ledger = new SchemaFailureLedger()) { this.ledger = ledger }
  options(): ArchiveRecordScanOptions {
    return { records: [["snapshot", "accounts", "*"], ["runtime_state", "strategy_states", "*"]], onRecord: row => this.row(row), onShape: shape => this.shape(shape) }
  }
  private attempt<Value>(stage: number, steps: (number | { key: string; insertion: number })[], operation: () => Value): { ok: true; value: Value } | { ok: false } {
    try { return { ok: true, value: operation() } }
    catch (error) { this.ledger.retain({ stage, steps }, error); return { ok: false } }
  }
  private shape(shape: ArchiveContainerShape): void {
    const name = shape.path.join(".")
    this.shapes.set(name, shape)
    if (shape.path.length === 1) {
      const stage = shape.path[0] === "snapshot" ? 18 : 27
      const keys = stage === 18 ? SAVE_SNAPSHOT_KEYS : SAVE_RUNTIME_KEYS
      this.attempt(stage, [0], () => exact(shapeRecord(shape), keys, String(shape.path[0])))
    } else if (shape.path.length === 2 && shape.kind !== "object" && shape.kind !== "number") {
      const stage = shape.path[0] === "snapshot" ? 18 : 27
      this.ledger.retain({ stage, steps: [2, 0] }, new SaveSchemaError(name, "必须是对象"))
    }
  }
  private row(row: ArchiveRecord): void {
    const snapshot = row.path[0] === "snapshot"
    const expected = snapshot ? "accounts" : "strategy_states"
    if (row.path.length !== 3 || row.path[1] !== expected || (row.path[0] !== "snapshot" && row.path[0] !== "runtime_state")) throw new Error("session schema sink 收到覆盖范围外的记录")
    const key = String(row.path[2]), path = `${row.path[0]}.${expected}.${key}`, stage = snapshot ? 18 : 27
    if (snapshot) this.accounts++
    else this.strategies++
    this.attempt(stage, [2, 1, { key, insertion: row.insertion }], () => {
      safeIntegerKey(key, path)
      const parsed = snapshot ? parseSaveAccount(row.value, path) : parseRuntimeStrategyState(row.value, path)
      this.attempt(49, [saveMaterializationOrder(snapshot ? "snapshot" : "runtime_state"), snapshot ? 3 : 5, { key, insertion: row.insertion }], () => materializeSaveValue(parsed, `根节点.${path}`))
    })
  }
  finish(scan: ArchiveRootScan) {
    const raw = scan.rawMetadata
    const rawSnapshot = Object.fromEntries(SAVE_SNAPSHOT_KEYS.map(key => [key, key === "accounts" ? {} : raw[`snapshot.${key}`]]))
    // 旧 snapshot 顺序是 markets→全部 accounts→seq/tick/candles；同一 codec 的两次小投影保留该顺序。
    this.attempt(18, [1], () => parseSaveSnapshot({ ...rawSnapshot, seq: 0, tick: 0, daily_candles: {}, active_daily_candles: {} }, "snapshot"))
    const snapshot = this.attempt(18, [3], () => parseSaveSnapshot(rawSnapshot, "snapshot"))
    const rawRuntime = Object.fromEntries(SAVE_RUNTIME_KEYS.map(key => [key, key === "strategy_states" ? {} : raw[`runtime_state.${key}`]]))
    const runtime = this.attempt(27, [1], () => parseSaveRuntime(rawRuntime))
    for (const [name, count] of [["snapshot.accounts", this.accounts], ["runtime_state.strategy_states", this.strategies]] as const) {
      const shape = this.shapes.get(name)
      if (shape?.kind === "object" && shape.length !== count) throw new Error(`${name} 分片覆盖不完整：${count}/${shape.length}`)
      // 数字 raw 表虽然 record 允许对象外壳，但 map 必须继续检查 token/decoded 两个 enumerable 键。
      if (shape?.kind === "number") this.attempt(name === "snapshot.accounts" ? 18 : 27, [2, 1], () => safeIntegerKey("token", `${name}.token`))
    }
    if (snapshot.ok) this.attempt(49, [saveMaterializationOrder("snapshot"), 0], () => materializeSaveValue(snapshot.value, "根节点.snapshot"))
    if (runtime.ok) this.attempt(49, [saveMaterializationOrder("runtime_state"), 0], () => materializeSaveValue(runtime.value, "根节点.runtime_state"))
    const coveredFields = ["snapshot", "runtime_state"].filter(name => this.shapes.has(name) && this.shapes.has(name === "snapshot" ? "snapshot.accounts" : "runtime_state.strategy_states"))
    return Object.freeze({ schemaValidated: false as const, accounts: this.accounts, strategies: this.strategies, snapshot, runtime, coveredFields: Object.freeze(coveredFields) })
  }
}
