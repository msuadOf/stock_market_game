import { saveMaterializationOrder } from "./materialization-order.ts"
import type { DailyCandle } from "../../types/generated/DailyCandle.ts"
import type { ArchiveContainerShape, ArchiveRecord, ArchiveRecordScanOptions, ArchiveRootScan } from "./archive-root-scan.ts"
import { parseFilledOrders, parseRestingOrders } from "./orders.ts"
import { materializeSaveValue, RawSaveNumber, SaveSchemaError } from "./primitives.ts"
import { nextRetainedDate, parseActiveMinuteBars, parseRetainedDay, validateRetainedCoverage, validateRetainedHistoryCandles, type HistoryContext } from "./retained-history.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"

export type MarketShardContext = HistoryContext & { readonly candles: Readonly<Record<string, readonly DailyCandle[]>> }
const STAGES = { resting_orders: 3, filled_orders: 5, market_minute_bars: 32, retained_market_days: 33 } as const
type MarketField = keyof typeof STAGES
function field(value: unknown): value is MarketField { return typeof value === "string" && Object.hasOwn(STAGES, value) }

/** 一次只持有一个证券订单桶或一个保留日，日K互证与链尾覆盖仍按原阶段仲裁。 */
export class MarketShardValidation {
  private context: MarketShardContext | undefined
  private expected: string | undefined
  private retainedDays = 0
  private counts: Partial<Record<MarketField, number>> = {}
  private shapes: Partial<Record<MarketField, ArchiveContainerShape>> = {}
  private hasRestingOrders = false
  private hasActiveMinutes = false
  readonly ledger: SchemaFailureLedger
  constructor(ledger: SchemaFailureLedger, context: MarketShardContext | undefined) { this.ledger = ledger; this.context = context; this.expected = context?.start_date }
  options(): ArchiveRecordScanOptions {
    return { records: Object.keys(STAGES).map(key => [key, "*"]), onRecord: row => this.row(row), onShape: shape => this.shape(shape) }
  }
  private shape(shape: ArchiveContainerShape): void {
    const name = shape.path[0]
    if (shape.path.length !== 1 || !field(name)) return
    this.shapes[name] = shape
    try {
      if (name === "retained_market_days") {
        if (shape.kind !== "array") throw new SaveSchemaError(name, "必须是数组")
      } else if (shape.kind !== "object") {
        const value = shape.kind === "number" ? new RawSaveNumber("0") : null
        if (name === "resting_orders") parseRestingOrders(value)
        else if (name === "filled_orders") parseFilledOrders(value)
        else parseActiveMinuteBars(value, name, this.context?.stocks, this.context)
      }
    } catch (error) { this.ledger.retain({ stage: STAGES[name], steps: [0] }, error) }
  }
  private row(row: ArchiveRecord): void {
    const name = row.path[0]
    if (row.path.length !== 2 || !field(name)) throw new Error("market schema sink 收到覆盖范围外的记录")
    this.counts[name] = (this.counts[name] ?? 0) + 1
    const key = String(row.path[1]), position = { key, insertion: row.insertion }
    const steps = name === "retained_market_days" ? [1, row.insertion] : [1, position]
    let parsed: unknown
    try {
      if (name === "retained_market_days") {
        if (typeof row.path[1] !== "number") return
        if (this.context === undefined || this.expected === undefined) return
        const day = parseRetainedDay(row.value, `retained_market_days[${row.path[1]}]`, this.expected, this.context)
        this.expected = nextRetainedDate(day.date)
        this.retainedDays++
        parsed = day
        try { validateRetainedHistoryCandles([day], this.context.candles) }
        catch (error) { this.ledger.retain({ stage: 34, steps: [row.path[1]] }, error) }
      } else if (name === "resting_orders") {
        const orders = parseRestingOrders({ [key]: row.value })
        this.hasRestingOrders ||= orders[key]!.length > 0
        parsed = orders
      } else if (name === "filled_orders") parsed = parseFilledOrders({ [key]: row.value })
      else {
        if (this.context === undefined) return
        const bars = parseActiveMinuteBars({ [key]: row.value }, name, this.context.stocks, this.context)
        this.hasActiveMinutes ||= bars[key]!.length > 0
        parsed = bars
      }
    } catch (error) { this.ledger.retain({ stage: STAGES[name], steps }, error); return }
    try { materializeSaveValue(parsed, name === "retained_market_days" ? `根节点.retained_market_days[${row.path[1]}]` : `根节点.${name}`) }
    catch (error) { this.ledger.retain({ stage: 49, steps: [saveMaterializationOrder(name), ...steps] }, error) }
  }
  finish(_scan: ArchiveRootScan) {
    for (const name of Object.keys(STAGES) as MarketField[]) {
      const shape = this.shapes[name], count = this.counts[name] ?? 0
      if ((shape?.kind === "object" && name !== "retained_market_days" || shape?.kind === "array" && name === "retained_market_days") && shape.length !== count) throw new Error(`${name} 分片覆盖不完整：${count}/${shape.length}`)
    }
    try { if (this.context !== undefined && this.expected !== undefined) validateRetainedCoverage(this.retainedDays, this.expected, this.context) }
    catch (error) { this.ledger.retain({ stage: 33, steps: [2] }, error) }
    return Object.freeze({ schemaValidated: false as const, retainedDays: this.retainedDays, hasRestingOrders: this.hasRestingOrders, hasActiveMinutes: this.hasActiveMinutes, coveredFields: Object.freeze(Object.keys(this.shapes).filter(name => (name !== "market_minute_bars" && name !== "retained_market_days") || this.context !== undefined)) })
  }
}
