import { saveMaterializationOrder } from "./materialization-order.ts"
import type { ArchiveContainerShape, ArchiveRecord, ArchiveRecordScanOptions, ArchiveRootScan } from "./archive-root-scan.ts"
import { array, exact, materializeSaveValue, record, SaveSchemaError } from "./primitives.ts"
import { parseOwnerSequence, parsePlanEntry, parsePlanPolicy, summarizePlanEntry, validatePlanBookIdentities, type PlanIdentitySummary } from "./personal/plans.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"

/** 全部 leaf 通过前不报告全书 cross-check；只保留 PlanId/股票/终止状态摘要。 */
export class PlanShardValidation {
  private summaries: PlanIdentitySummary[] = []
  private plans = 0
  private tableSeen = false
  private hasIntradayExecution = false
  private planShape?: ArchiveContainerShape
  readonly ledger: SchemaFailureLedger
  constructor(ledger = new SchemaFailureLedger()) { this.ledger = ledger }
  options(): ArchiveRecordScanOptions {
    return { records: [["plans", "plans", "*"]], onRecord: row => this.row(row), onShape: shape => this.shape(shape) }
  }
  private shape(shape: ArchiveContainerShape): void {
    if (shape.path.length === 1) {
      this.tableSeen = true
      try {
        if (shape.kind !== "object" && shape.kind !== "number") record(null, "plans")
        exact(Object.fromEntries(shape.keys.map(key => [key, undefined])), ["policy", "owner_sequences", "plans"], "plans")
      } catch (error) { this.ledger.retain({ stage: 42, steps: [0] }, error) }
    } else if (shape.path.length === 2) {
      this.planShape = shape
      if (shape.kind !== "array") this.ledger.retain({ stage: 42, steps: [3, 0] }, new SaveSchemaError("plans.plans", "必须是数组"))
    }
  }
  private row(row: ArchiveRecord): void {
    if (row.path.length !== 3 || row.path[0] !== "plans" || row.path[1] !== "plans") throw new Error("plan schema sink 收到覆盖范围外的记录")
    if (typeof row.path[2] !== "number") return
    const index = row.path[2], path = `plans.plans[${index}]`
    this.plans++
    try {
      const entry = parsePlanEntry(row.value, path)
      this.summaries.push(summarizePlanEntry(entry, index))
      if ("Investor" in entry ? entry.Investor.execution !== undefined || entry.Investor.active_child_order_id !== null : entry.IssuerRepurchase.execution !== null) this.hasIntradayExecution = true
      try { materializeSaveValue(entry, `根节点.${path}`) }
      catch (error) { this.ledger.retain({ stage: 49, steps: [saveMaterializationOrder("plans"), 2, index] }, error) }
    } catch (error) { this.ledger.retain({ stage: 42, steps: [3, 1, index] }, error) }
  }
  finish(scan: ArchiveRootScan) {
    if (this.planShape?.kind === "array" && this.planShape.length !== this.plans) throw new Error(`plans.plans 分片覆盖不完整：${this.plans}/${this.planShape.length}`)
    const raw = scan.rawMetadata
    try { parsePlanPolicy(raw["plans.policy"], "plans.policy") }
    catch (error) { this.ledger.retain({ stage: 42, steps: [1] }, error) }
    let owners: ReturnType<typeof parseOwnerSequence>[] | undefined
    try { owners = array(raw["plans.owner_sequences"], "plans.owner_sequences").map((value, index) => parseOwnerSequence(value, `plans.owner_sequences[${index}]`)) }
    catch (error) { this.ledger.retain({ stage: 42, steps: [2] }, error) }
    if (owners !== undefined) {
      try { validatePlanBookIdentities(owners, this.summaries) }
      catch (error) { this.ledger.retain({ stage: 42, steps: [4] }, error) }
    }
    return Object.freeze({ schemaValidated: false as const, plans: this.plans, hasIntradayExecution: this.hasIntradayExecution, coveredFields: Object.freeze(this.tableSeen && this.planShape !== undefined ? ["plans"] : []) })
  }
}
