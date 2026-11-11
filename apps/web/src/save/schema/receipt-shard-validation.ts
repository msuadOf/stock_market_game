import { saveMaterializationOrder } from "./materialization-order.ts"
import type { ArchiveContainerShape, ArchiveRecord, ArchiveRecordScanOptions, ArchiveRootScan } from "./archive-root-scan.ts"
import { materializeSaveValue, RawSaveNumber, SaveSchemaError } from "./primitives.ts"
import { parsePersonalTradeConfirmations, parseTradeReceipt, validateReceiptWatermarkMaximum } from "./retained-history.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"

export type ReceiptShardContext = Readonly<{ accounts: readonly string[]; stocks: readonly string[]; startDate: string; currentDate: string }>
type AccountSummary = { insertion: number; count: number; previous?: bigint }

/** 每账户只留上一 id；跨域检查使用首遍验证过的目录，不保留 receipt 原行。 */
export class ReceiptShardValidation {
  private accounts = new Map<string, AccountSummary>()
  private maximum?: bigint
  private rows = 0
  private tableShape?: ArchiveContainerShape
  private accountKeys: ReadonlySet<string>
  private stockKeys: ReadonlySet<string>
  private context: ReceiptShardContext
  readonly ledger: SchemaFailureLedger
  constructor(ledger: SchemaFailureLedger, context: ReceiptShardContext) {
    this.ledger = ledger
    this.accountKeys = new Set(context.accounts)
    this.stockKeys = new Set(context.stocks)
    this.context = Object.freeze({ ...context, accounts: Object.freeze([...context.accounts]), stocks: Object.freeze([...context.stocks]) })
  }
  options(): ArchiveRecordScanOptions {
    return { records: [["personal_trade_confirmations", "*", "*"]], onRecord: row => this.row(row), onShape: shape => this.shape(shape) }
  }
  private account(key: string): AccountSummary {
    let summary = this.accounts.get(key)
    if (summary === undefined) { summary = { insertion: this.accounts.size, count: 0 }; this.accounts.set(key, summary) }
    return summary
  }
  private shape(shape: ArchiveContainerShape): void {
    if (shape.path.length === 1) {
      this.tableShape = shape
      if (shape.kind !== "object") {
        try { parsePersonalTradeConfirmations(shape.kind === "number" ? new RawSaveNumber("0") : null) }
        catch (error) { this.ledger.retain({ stage: 35, steps: [0] }, error) }
      }
      return
    }
    if (shape.path.length !== 2) return
    const key = String(shape.path[1]), summary = this.account(key), position = { key, insertion: summary.insertion }, accountPath = `personal_trade_confirmations.${key}`
    if (!/^\d+$/.test(key)) this.ledger.retain({ stage: 35, steps: [1, position, 0] }, new SaveSchemaError(accountPath, "键必须为数值 AccountId 的字符串形态"))
    if (shape.kind !== "array") this.ledger.retain({ stage: 35, steps: [1, position, 1] }, new SaveSchemaError(accountPath, "必须是数组"))
    else if (summary.count !== shape.length) throw new Error(`${accountPath} 分片覆盖不完整：${summary.count}/${shape.length}`)
    if (!this.accountKeys.has(key)) this.ledger.retain({ stage: 36, steps: [position, 0] }, new Error(`存档 ${accountPath} 账户不存在`))
  }
  private row(row: ArchiveRecord): void {
    if (row.path.length !== 3 || row.path[0] !== "personal_trade_confirmations") throw new Error("receipt schema sink 收到覆盖范围外的记录")
    // 非数组账户值由 shape 拒绝；对象属性不能冒充 receipt 数组行。
    if (typeof row.path[2] !== "number") return
    const key = String(row.path[1]), summary = this.account(key), position = { key, insertion: summary.insertion }, index = row.path[2], path = `personal_trade_confirmations.${key}[${index}]`
    summary.count++; this.rows++
    let receipt: ReturnType<typeof parseTradeReceipt>
    try { receipt = parseTradeReceipt(row.value, path) }
    catch (error) { this.ledger.retain({ stage: 35, steps: [1, position, 1, index] }, error); return }
    const id = BigInt(receipt.receipt_id)
    if (summary.previous !== undefined && summary.previous >= id) this.ledger.retain({ stage: 35, steps: [1, position, 2, index] }, new SaveSchemaError(path, "receipt_id 必须严格递增"))
    summary.previous = id
    if (this.maximum === undefined || id > this.maximum) this.maximum = id
    if (!this.stockKeys.has(String(receipt.stock)) || receipt.civil_date < this.context.startDate || receipt.civil_date > this.context.currentDate) this.ledger.retain({ stage: 36, steps: [position, 1, index] }, new Error(`存档 ${path} 证券或成交自然日不属于当前会话`))
    try { materializeSaveValue(receipt, `根节点.${path}`) }
    catch (error) { this.ledger.retain({ stage: 49, steps: [saveMaterializationOrder("personal_trade_confirmations"), position, index] }, error) }
  }
  finish(scan: ArchiveRootScan) {
    if (this.tableShape?.kind === "object" && this.tableShape.length !== this.accounts.size) throw new Error("personal_trade_confirmations 账户分片覆盖不完整")
    let watermark: number | undefined
    try { watermark = validateReceiptWatermarkMaximum(scan.rawMetadata.receipt_watermark, this.maximum) }
    catch (error) { this.ledger.retain({ stage: 37, steps: [] }, error) }
    return Object.freeze({ schemaValidated: false as const, rows: this.rows, accounts: this.accounts.size, maximumReceiptId: this.maximum?.toString(), watermark, coveredFields: Object.freeze(this.tableShape === undefined ? [] : ["personal_trade_confirmations", "receipt_watermark"]) })
  }
}
