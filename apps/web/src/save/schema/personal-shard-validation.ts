import { saveMaterializationOrder } from "./materialization-order.ts"
import type { ArchiveRecordScanOptions, ArchiveRecord, ArchiveContainerShape } from "./archive-root-scan.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"
import { map, materializeSaveValue, safeIntegerKey, RawSaveNumber, SaveSchemaError } from "./primitives.ts"
import { accountKey } from "./personal/common.ts"
import { parseNpcAttention } from "./npc-attention.ts"
import { parseRetailExperienceStates } from "./personal/experience.ts"
import { parseInformationState, parseInformationStates } from "./personal/information.ts"
import { parseBeliefBook, parseBeliefBooks } from "./personal/beliefs.ts"
import { parseWatchlists, parsePriceMemories, parseHistoryReads } from "./personal/memory.ts"

// 与现行 ROOT_KEYS 对照；pending 不能被解释为已验证或空字段。
export const SHARDED_SCHEMA_COVERAGE = Object.freeze({
  desk_mirrored_resting: "metadata", runtime_state: "session", setup: "metadata", seed: "metadata", snapshot: "session",
  resting_orders: "market", book_next_sequences: "metadata", filled_orders: "market", price_history: "metadata", market_minute_closes: "metadata",
  market_minute_bars: "market", retained_market_days: "market", personal_trade_confirmations: "receipts", receipt_watermark: "receipts", rng_state: "metadata",
  npc_attention: "personal", retail_experience: "personal", npc_order_lifecycles: "metadata", pending_player: "metadata", ingress_receipt_cursors: "metadata", next_order_id: "metadata",
  civil_clock: "metadata", company_state: "metadata", report_correction_operations: "metadata", public_library: "metadata", disclosures: "metadata", plans: "plans", urgency_policy: "metadata",
  information_states: "personal", belief_books: "personal", watchlists: "personal", price_memories: "personal", history_reads: "personal", market_memberships: "metadata",
} as const)

export const PERSONAL_SCHEMA_FIELDS = Object.freeze(["npc_attention", "retail_experience", "information_states", "belief_books", "watchlists", "price_memories", "history_reads"] as const)
export type PersonalSchemaField = typeof PERSONAL_SCHEMA_FIELDS[number]

// 原 root.ts 的顺序：npc_attention 位于 parseOrderState，其余个人表位于最终返回投影。
const STAGES: Record<PersonalSchemaField, number> = { npc_attention: 8, retail_experience: 40, information_states: 44, belief_books: 45, watchlists: 46, price_memories: 47, history_reads: 48 }
const TABLE_PARSERS: Record<PersonalSchemaField, (value: unknown) => unknown> = {
  npc_attention: value => map(value, "npc_attention", safeIntegerKey, parseNpcAttention),
  retail_experience: parseRetailExperienceStates, information_states: parseInformationStates, belief_books: parseBeliefBooks,
  watchlists: parseWatchlists, price_memories: parsePriceMemories, history_reads: parseHistoryReads,
}
function personalField(value: unknown): value is PersonalSchemaField {
  return typeof value === "string" && Object.hasOwn(STAGES, value)
}

/** INFO/Book 的 parsed 是单行，其余 codec 返回单键表；保留整档物化的原路径。 */
export function materializePersonalShard(field: PersonalSchemaField, key: string, parsed: unknown): unknown {
  const path = field === "information_states" || field === "belief_books" ? `根节点.${field}.${key}` : `根节点.${field}`
  return materializeSaveValue(parsed, path)
}

/** 只收集个人字段；完整 root、跨域、日终与 engine 恢复仍未覆盖。 */
export class PersonalShardValidation {
  readonly ledger: SchemaFailureLedger
  private counts: Partial<Record<PersonalSchemaField, number>> = {}
  private shapes: Partial<Record<PersonalSchemaField, ArchiveContainerShape>> = {}
  constructor(ledger = new SchemaFailureLedger()) { this.ledger = ledger }
  options(): ArchiveRecordScanOptions {
    return { records: PERSONAL_SCHEMA_FIELDS.map(field => [field, "*"]), onRecord: value => { this.record(value) }, onShape: shape => { this.shape(shape) } }
  }
  private shape(shape: ArchiveContainerShape): void {
    const field = shape.path[0]
    if (shape.path.length !== 1 || !personalField(field)) return
    this.shapes[field] = shape
    if (shape.kind === "number") {
      // 旧 raw parser 的数字是对象外壳，map 仍应报 token 键错误，不能提前改成对象类型错。
      try { TABLE_PARSERS[field](new RawSaveNumber("0")) }
      catch (error) { this.ledger.retain({ stage: STAGES[field], steps: [0] }, error) }
    } else if (shape.kind !== "object") this.ledger.retain({ stage: STAGES[field], steps: [0] }, new SaveSchemaError(field, "必须是对象"))
  }
  private record(record: ArchiveRecord): void {
    const field = record.path[0]
    if (record.path.length !== 2 || !personalField(field)) throw new Error("个人 schema sink 收到覆盖范围外的记录")
    const key = String(record.path[1]), path = `${field}.${key}`
    const position = { key, insertion: record.insertion }
    this.counts[field] = (this.counts[field] ?? 0) + 1
    let parsed: unknown
    try {
      if (field === "information_states" || field === "belief_books") {
        accountKey(key, path)
        parsed = field === "information_states" ? parseInformationState(record.value, path) : parseBeliefBook(record.value, path)
      } else parsed = TABLE_PARSERS[field]({ [key]: record.value })
    } catch (error) {
      this.ledger.retain({ stage: STAGES[field], steps: [1, position] }, error)
      return
    }
    if (field === "information_states" || field === "belief_books") {
      const identity = field === "information_states" ? (parsed as { owner: string }).owner : (parsed as { npc: string }).npc
      if (identity !== key) {
        // 复用原整表 codec 的 post-check 错误；phase 2 保证晚于整张表所有 leaf。
        try { TABLE_PARSERS[field]({ [key]: record.value }) }
        catch (error) { this.ledger.retain({ stage: STAGES[field], steps: [2, position] }, error) }
      }
    }
    try { materializePersonalShard(field, key, parsed) }
    catch (error) {
      // 原物化守卫位于完整 root schema 之后，不能抢占其他字段的 schema 首错。
      this.ledger.retain({ stage: 49, steps: [saveMaterializationOrder(field), position] }, error)
    }
  }
  finish(): Readonly<{ schemaValidated: false; counts: Readonly<Partial<Record<PersonalSchemaField, number>>> }> {
    const counts: Partial<Record<PersonalSchemaField, number>> = {}
    for (const field of PERSONAL_SCHEMA_FIELDS) {
      const shape = this.shapes[field]
      if (shape === undefined) continue
      const seen = this.counts[field] ?? 0
      if (shape.kind === "object" && seen !== shape.length) throw new Error(`${field} 分片覆盖不完整：${seen}/${shape.length}`)
      counts[field] = seen
    }
    return Object.freeze({ schemaValidated: false, counts: Object.freeze(counts) })
  }
}
