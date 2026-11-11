import { createUtf8Sha256Chain, type Sha256ChainResult, type Utf8Sha256Chain } from "./sha256-chain.ts"
import { readCapturedArchiveChunks, type CapturedArchiveSource } from "./archive-source.ts"
import { validateDayEndArchiveFacts } from "./day-end-candidate.ts"
import { scanArchiveRoot, type ArchiveContainerShape, type ArchiveRecord, type ArchiveRecordScanOptions } from "./schema/archive-root-scan.ts"
import { MarketShardValidation, type MarketShardContext } from "./schema/market-shard-validation.ts"
import { PersonalShardValidation, SHARDED_SCHEMA_COVERAGE } from "./schema/personal-shard-validation.ts"
import { PlanShardValidation } from "./schema/plan-shard-validation.ts"
import { ReceiptShardValidation, type ReceiptShardContext } from "./schema/receipt-shard-validation.ts"
import { validateRootMetadata } from "./schema/root-metadata-validation.ts"
import { SessionShardValidation } from "./schema/session-shard-validation.ts"
import { SchemaFailureLedger } from "./schema/shard-errors.ts"
import { ROOT_KEYS } from "./schema/root.ts"
import { exact } from "./schema/primitives.ts"
import { validateMembershipAccounts } from "./schema/market-memberships.ts"

function matches(path: readonly (string | number)[], pattern: readonly string[], prefix: boolean): boolean {
  return (prefix ? path.length < pattern.length : path.length === pattern.length) && path.every((value, index) => pattern[index] === "*" || pattern[index] === value)
}
function combine(sinks: readonly ArchiveRecordScanOptions[]): ArchiveRecordScanOptions {
  return {
    records: sinks.flatMap(sink => sink.records),
    onRecord: (row: ArchiveRecord) => { for (const sink of sinks) if (sink.records.some(pattern => matches(row.path, pattern, false))) sink.onRecord(row) },
    onShape: (shape: ArchiveContainerShape) => { for (const sink of sinks) if (sink.records.some(pattern => matches(shape.path, pattern, true))) sink.onShape?.(shape) },
  }
}

/** Web Crypto 不提供取消；及时结束本次等待，并继续观察原生 Promise 的晚到结果。 */
async function waitDigest<Value>(operation: () => Promise<Value>, signal?: AbortSignal): Promise<Value> {
  signal?.throwIfAborted()
  const pending = operation()
  if (signal === undefined) return pending
  let remove: (() => void) | undefined
  try {
    return await new Promise<Value>((resolve, reject) => {
      const abort = () => { reject(signal.reason) }
      remove = () => { signal.removeEventListener("abort", abort) }
      signal.addEventListener("abort", abort, { once: true })
      // 两个分支均注册，取消后晚到的原生拒绝也不会形成 unhandled rejection。
      pending.then(resolve, reject)
      if (signal.aborted) abort()
    })
  } finally { remove?.() }
}

/** 摘要故障先锁存并排空原流，保留 gzip/词法/取消原有优先级；失败绝不补默认摘要。 */
async function scanDigestPass(source: CapturedArchiveSource, signal?: AbortSignal, options?: ArchiveRecordScanOptions) {
  signal?.throwIfAborted()
  let hash: Utf8Sha256Chain | undefined
  let digestFailure: { error: unknown } | undefined
  let contentDigest: Sha256ChainResult | undefined
  try { hash = await waitDigest(() => createUtf8Sha256Chain(), signal) }
  catch (error) { digestFailure = { error } }
  signal?.throwIfAborted()
  async function* input() {
    for await (const chunk of readCapturedArchiveChunks(source, undefined, signal)) {
      // 用 break 关闭上游，避免 AsyncIteratorClose 的原 throw 吞掉 return/finally 清理错误。
      if (signal?.aborted) break
      if (hash !== undefined && digestFailure === undefined) {
        try { const activeHash = hash; await waitDigest(() => activeHash.update(chunk), signal) }
        catch (error) { digestFailure = { error } }
      }
      if (signal?.aborted) break
      yield chunk
    }
    signal?.throwIfAborted()
    if (hash !== undefined && digestFailure === undefined) {
      try { const activeHash = hash; contentDigest = await waitDigest(() => activeHash.finish(), signal) }
      catch (error) { digestFailure = { error } }
    }
    signal?.throwIfAborted()
  }
  const scan = await scanArchiveRoot(input(), options)
  signal?.throwIfAborted()
  if (digestFailure !== undefined) throw new Error("计算存档内容摘要失败", { cause: digestFailure.error })
  if (contentDigest === undefined || contentDigest.byteLength !== BigInt(scan.utf8Bytes)) throw new Error("存档内容摘要覆盖或 UTF-8 字节数门禁未通过")
  return { scan, contentDigest }
}

function sameDigest(left: Sha256ChainResult, right: Sha256ChainResult): boolean {
  return left.algorithm === right.algorithm && left.blockBytes === right.blockBytes && left.byteLength === right.byteLength && left.blocks === right.blocks && left.digest === right.digest
}

/** 仅用于完整验证报告；尚不签发 Memory admission、WASM token 或可恢复租约。 */
export async function validateCapturedArchiveSchema(source: CapturedArchiveSource, signal?: AbortSignal) {
  const firstPass = await scanDigestPass(source, signal)
  const first = firstPass.scan
  const ledger = new SchemaFailureLedger()
  const metadata = validateRootMetadata(first, ledger)
  // 首遍没有账户/策略 row sink；此投影仅用于构造后续 context，不记为完整字段覆盖。
  const projection = new SessionShardValidation(ledger).finish(first)
  const accounts = first.keySets["snapshot.accounts"]
  let marketContext: MarketShardContext | undefined
  let receiptContext: ReceiptShardContext | undefined
  if (metadata.setup.ok && metadata.clock.ok && projection.snapshot.ok && first.keySetKinds["snapshot.accounts"] === "object" && accounts !== undefined) {
    const setup = metadata.setup.value, clock = metadata.clock.value, snapshot = projection.snapshot.value
    const sorted = [...setup.stocks].sort((left, right) => left.code.localeCompare(right.code))
    marketContext = { stocks: sorted.map((stock, index) => ({ id: String(index), tick: stock.tick })), start_date: setup.start_date, settled_through: clock.settled_through, auction_ticks: setup.auction_ticks, closing_auction_ticks: setup.closing_auction_ticks, candles: snapshot.daily_candles }
    receiptContext = { accounts, stocks: Object.keys(snapshot.markets), startDate: setup.start_date, currentDate: clock.current_date }
  }
  const personal = new PersonalShardValidation(ledger), session = new SessionShardValidation(ledger), plans = new PlanShardValidation(ledger), market = new MarketShardValidation(ledger, marketContext)
  const receipts = receiptContext === undefined ? undefined : new ReceiptShardValidation(ledger, receiptContext)
  const sinks = [personal.options(), session.options(), plans.options(), market.options()]
  if (receipts !== undefined) sinks.push(receipts.options())
  const secondPass = await scanDigestPass(source, signal, combine(sinks))
  const second = secondPass.scan
  if (!sameDigest(firstPass.contentDigest, secondPass.contentDigest)) throw new Error("同一捕获源的两遍内容摘要不一致")
  if (first.utf8Bytes !== second.utf8Bytes || first.utf16Characters !== second.utf16Characters || first.rootKeys.length !== second.rootKeys.length || first.rootKeys.some((key, index) => key !== second.rootKeys[index])) throw new Error("同一捕获源的两遍词法扫描不一致")
  const personalResult = personal.finish(), sessionResult = session.finish(second), planResult = plans.finish(second), marketResult = market.finish(second), receiptResult = receipts?.finish(second)
  const retain = (stage: number, operation: () => unknown) => { try { operation() } catch (error) { ledger.retain({ stage, steps: [] }, error) } }
  if (metadata.setup.ok && metadata.memberships.ok && accounts !== undefined && first.keySetKinds["snapshot.accounts"] === "object") {
    const setup = metadata.setup.value, memberships = metadata.memberships.value
    retain(26, () => validateMembershipAccounts(memberships, setup, { accounts: Object.fromEntries(accounts.map(key => [key, undefined])) }))
  }
  if (metadata.setup.ok && sessionResult.runtime.ok) {
    const expectedStockIds = metadata.setup.value.stocks.map((_, index) => String(index))
    const cutoff = sessionResult.runtime.value.book_public_cutoff_sequences
    retain(28, () => exact(cutoff, expectedStockIds, "runtime_state.book_public_cutoff_sequences (setup.stocks→StockId)"))
    if (metadata.bookNextSequences.ok) {
      const next = metadata.bookNextSequences.value
      retain(29, () => { for (const [stock, sequence] of Object.entries(cutoff)) if (BigInt(sequence) > BigInt(next[stock]!)) throw new Error(`存档 runtime_state.book_public_cutoff_sequences.${stock} 超过 book_next_sequences`) })
    }
  }
  // 先仲裁完整 schema/跨域/物化首错；上下文前件失败绝不能靠空 context 获得覆盖。
  ledger.throwIfAny()
  const coveredFields = [...metadata.coveredFields, ...Object.keys(personalResult.counts), ...sessionResult.coveredFields, ...planResult.coveredFields, ...marketResult.coveredFields, ...(receiptResult?.coveredFields ?? [])]
  if (coveredFields.length !== ROOT_KEYS.length || new Set(coveredFields).size !== ROOT_KEYS.length || ROOT_KEYS.some(key => !coveredFields.includes(key)) || Object.keys(SHARDED_SCHEMA_COVERAGE).some(key => !ROOT_KEYS.includes(key as typeof ROOT_KEYS[number]))) throw new Error("完整存档 schema 字段覆盖门禁未通过")
  if (!metadata.setup.ok || !metadata.seed.ok || !metadata.clock.ok || !metadata.pendingPlayer.ok || !metadata.lifecycles.ok || !sessionResult.snapshot.ok || !sessionResult.runtime.ok || receipts === undefined || marketContext === undefined) throw new Error("完整存档 schema context 门禁未通过")
  validateDayEndArchiveFacts({
    settledThrough: metadata.clock.value.settled_through,
    tick: sessionResult.snapshot.value.tick,
    ticksPerDay: metadata.setup.value.ticks_per_day,
    hasPendingPlayer: metadata.pendingPlayer.value.length > 0,
    hasIntradayOrders: marketResult.hasRestingOrders || planResult.hasIntradayExecution || sessionResult.runtime.value.live_envelopes.length > 0 || metadata.lifecycles.value.length > 0,
    hasActiveMinutes: marketResult.hasActiveMinutes,
  })
  signal?.throwIfAborted()
  return Object.freeze({ source, schemaValidated: true as const, dayEndValidated: true as const, productionAdmission: false as const, utf8Bytes: first.utf8Bytes, utf16Characters: first.utf16Characters, coveredFields: Object.freeze([...coveredFields]), contentDigest: firstPass.contentDigest, setup: metadata.setup.value, seed: metadata.seed.value })
}
