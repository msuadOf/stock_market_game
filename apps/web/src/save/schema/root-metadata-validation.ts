import { saveMaterializationOrder } from "./materialization-order.ts"
import type { ArchiveRootScan } from "./archive-root-scan.ts"
import { SchemaFailureLedger } from "./shard-errors.ts"
import { ROOT_KEYS, parseReportCorrectionOperations } from "./root.ts"
import { boolean, decimal, exact, integer, map, record, materializeSaveValue, SaveSchemaError } from "./primitives.ts"
import { parseSetup } from "./market.ts"
import { parseCivilClock } from "./civil-clock.ts"
import { parseMarketMemberships } from "./market-memberships.ts"
import { parseUrgencyPolicy } from "./urgency-policy.ts"
import { stockKey } from "./personal/common.ts"

import { parsePriceHistory, parseMarketMinuteCloses, parseNpcOrderLifecycles, parsePendingPlayer, parseIngressReceiptCursors } from "./orders.ts"
import { parseSavedSessionCompanies } from "./company/state.ts"
import { parsePublicLibrary, parseDisclosureDispatch } from "./company/index.ts"

type Attempt<Value> = { ok: true; value: Value } | { ok: false }

/** 元数据字段通过仍不代表完整 root 或跨域校验通过。 */
export function validateRootMetadata(scan: ArchiveRootScan, ledger: SchemaFailureLedger) {
  const attempt = <Value>(stage: number, operation: () => Value): Attempt<Value> => {
    try { return { ok: true, value: operation() } }
    catch (error) { ledger.retain({ stage, steps: [] }, error); return { ok: false } }
  }
  // 旧 JSON parser 的数字根是 RawSaveNumber 对象：先报 root 必填字段，不能改成对象类型错。
  if (scan.rootKind !== "object" && scan.rootKind !== "number") attempt(0, () => record(null, "根节点"))
  const header = Object.fromEntries(scan.rootKeys.map(key => [key, undefined]))
  if ("pending_npc" in header) ledger.retain({ stage: 1, steps: [] }, new Error("旧格式存档（含 pending_npc 键）不被当前版本接受：请用当前引擎重新生成存档"))
  attempt(2, () => exact(header, ROOT_KEYS, "根节点"))
  const raw = scan.rawMetadata
  const bookNextSequences = attempt(4, () => map(raw.book_next_sequences, "book_next_sequences", stockKey, decimal))
  const priceHistory = attempt(6, () => parsePriceHistory(raw.price_history))
  const minuteCloses = attempt(7, () => parseMarketMinuteCloses(raw.market_minute_closes))
  const lifecycles = attempt(9, () => parseNpcOrderLifecycles(raw.npc_order_lifecycles))
  const pendingPlayer = attempt(10, () => parsePendingPlayer(raw.pending_player))
  const ingressCursors = attempt(11, () => parseIngressReceiptCursors(raw.ingress_receipt_cursors))
  const rng = attempt(14, () => decimal(raw.rng_state, "rng_state"))
  const nextOrderId = attempt(15, () => integer(raw.next_order_id, "next_order_id", 1))
  const setup = attempt(16, () => parseSetup(raw.setup, "setup"))
  if (setup.ok && setup.value.simulation_policy_id !== "a-share-simulation") {
    ledger.retain({ stage: 17, steps: [] }, new Error(`存档 setup.simulation_policy_id 必须为 a-share-simulation，实际为 ${JSON.stringify(setup.value.simulation_policy_id)}`))
  }
  const companies = setup.ok ? attempt(19, () => parseSavedSessionCompanies(raw.company_state, setup.value)) : { ok: false } as const
  const corrections = attempt(20, () => parseReportCorrectionOperations(raw.report_correction_operations))
  const library = attempt(21, () => parsePublicLibrary(raw.public_library))
  if (companies.ok && library.ok) attempt(22, () => {
    const selectedReportSource = "LegacyOperations" in companies.value ? "SimulationAccounting" : "SimpleGenerated"
    for (const [index, report] of library.value.reports.entries()) {
      if (report.source !== selectedReportSource) throw new SaveSchemaError(`public_library.reports[${index}].source`, "必须来自当前 selected 公司权威")
    }
  })
  if (bookNextSequences.ok && setup.ok) attempt(23, () => exact(bookNextSequences.value, [...setup.value.stocks].sort((left, right) => left.code.localeCompare(right.code)).map((_, index) => String(index)), "book_next_sequences (setup.stocks→StockId)"))
  // snapshot.markets 的完整 leaf 校验属于 snapshot 阶段；这里只复用真实对象键。
  const markets = raw["snapshot.markets"]
  if (bookNextSequences.ok && typeof markets === "object" && markets !== null && !Array.isArray(markets)) attempt(24, () => exact(bookNextSequences.value, Object.keys(markets), "book_next_sequences (snapshot.markets)"))
  const memberships = attempt(25, () => parseMarketMemberships(raw.market_memberships))
  const accountKeys = scan.keySets["snapshot.accounts"]
  const historyKeys = scan.keySets.history_reads
  if (scan.keySetKinds["snapshot.accounts"] === "object" && accountKeys !== undefined && historyKeys !== undefined) attempt(30, () => {
    const historyHeader = scan.keySetKinds.history_reads === "null" ? null : Object.fromEntries(historyKeys.map(key => [key, undefined]))
    exact(historyHeader as Record<string, unknown>, accountKeys, "history_reads (snapshot.accounts)")
  })
  const clock = attempt(31, () => parseCivilClock(raw.civil_clock))
  const deskMirroredResting = attempt(38, () => boolean(raw.desk_mirrored_resting, "desk_mirrored_resting"))
  const seed = attempt(39, () => decimal(raw.seed, "seed"))
  const disclosures = attempt(41, () => parseDisclosureDispatch(raw.disclosures))
  const urgency = attempt(43, () => parseUrgencyPolicy(raw.urgency_policy))
  const parsedFields: Readonly<Record<string, Attempt<unknown>>> = {
    setup, book_next_sequences: bookNextSequences, market_memberships: memberships, civil_clock: clock, seed, urgency_policy: urgency, rng_state: rng,
    next_order_id: nextOrderId, desk_mirrored_resting: deskMirroredResting, price_history: priceHistory, market_minute_closes: minuteCloses,
    npc_order_lifecycles: lifecycles, pending_player: pendingPlayer, ingress_receipt_cursors: ingressCursors, company_state: companies,
    report_correction_operations: corrections, public_library: library, disclosures,
  }
  for (const [field, result] of Object.entries(parsedFields)) if (result.ok) {
    try { materializeSaveValue(result.value, `根节点.${field}`) }
    catch (error) { ledger.retain({ stage: 49, steps: [saveMaterializationOrder(field)] }, error) }
  }
  const coveredFields = Object.keys(parsedFields).filter(field => parsedFields[field]!.ok)
  return Object.freeze({ schemaValidated: false as const, setup, bookNextSequences, memberships, clock, seed, urgency, rng, nextOrderId, deskMirroredResting, priceHistory, minuteCloses, lifecycles, pendingPlayer, ingressCursors, companies, corrections, library, disclosures, coveredFields: Object.freeze(coveredFields) })
}
