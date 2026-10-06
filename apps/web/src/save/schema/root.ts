import { parseDisclosureDispatch, parsePublicLibrary } from "./company/index.ts"
import { parseCompanySystemState, validateCompanySystemSession } from "./company/system.ts"
import { parseSetup } from "./market.ts"
import { parseSaveSnapshot, validateCashExReferenceFacts } from "./save-snapshot.ts"
import { parseOrderState } from "./orders.ts"
import { parseBeliefBooks } from "./personal/beliefs.ts"
import { parseRetailExperienceStates } from "./personal/experience.ts"
import { parseInformationStates } from "./personal/information.ts"
import { parseHistoryReads, parsePriceMemories, parseWatchlists } from "./personal/memory.ts"
import { parsePendingPlanEvents, parsePlanBook } from "./personal/plans.ts"
import { SaveSchemaError, decimal, exact, record } from "./primitives.ts"
import { parseCivilClock } from "./civil-clock.ts"
import { parseSaveRuntime } from "./runtime-state.ts"
import { parseUrgencyPolicy } from "./urgency-policy.ts"
import { parseReportCorrections, validateReportCorrectionLinks } from "./company/report-corrections.ts"
import { parseSessionCorporateActions } from "./corporate-actions.ts"

import { parseMarketMemberships, validateMembershipAccounts } from "./market-memberships.ts"
import { parseMinuteBars, parseRetainedHistory, validateRetainedHistoryCandles } from "./retained-history.ts"

export type StrictSaveEnvelope = {
  readonly retained_market_history: ReturnType<typeof parseRetainedHistory>
  readonly market_memberships: ReturnType<typeof parseMarketMemberships>
  readonly report_correction_operations: ReturnType<typeof parseReportCorrections>
  readonly runtime_state: ReturnType<typeof parseSaveRuntime>
  readonly setup: ReturnType<typeof parseSetup>
  readonly seed: string
  readonly snapshot: ReturnType<typeof parseSaveSnapshot>
  readonly auction_orders: ReturnType<typeof parseOrderState>["auction_orders"]
  readonly book_next_sequences: ReturnType<typeof parseOrderState>["book_next_sequences"]
  readonly resting_orders: ReturnType<typeof parseOrderState>["resting_orders"]
  readonly price_history: ReturnType<typeof parseOrderState>["price_history"]
  readonly market_minute_closes: ReturnType<typeof parseOrderState>["market_minute_closes"]
  readonly rng_state: string
  readonly npc_attention: ReturnType<typeof parseOrderState>["npc_attention"]
  readonly retail_experience: ReturnType<typeof parseRetailExperienceStates>
  readonly parent_orders: ReturnType<typeof parseOrderState>["parent_orders"]
  readonly npc_order_lifecycles: ReturnType<typeof parseOrderState>["npc_order_lifecycles"]
  readonly pending_player: ReturnType<typeof parseOrderState>["pending_player"]
  readonly pending_npc: ReturnType<typeof parseOrderState>["pending_npc"]
  readonly ingress_receipt_cursors: ReturnType<typeof parseOrderState>["ingress_receipt_cursors"]
  readonly next_order_id: number
  readonly civil_clock: ReturnType<typeof parseCivilClock>
  readonly company_system: ReturnType<typeof parseCompanySystemState>
  readonly corporate_actions: ReturnType<typeof parseSessionCorporateActions>
  readonly public_library: ReturnType<typeof parsePublicLibrary>
  readonly disclosures: ReturnType<typeof parseDisclosureDispatch>
  readonly plans: ReturnType<typeof parsePlanBook>
  readonly urgency_policy: ReturnType<typeof parseUrgencyPolicy>
  readonly information_states: ReturnType<typeof parseInformationStates>
  readonly belief_books: ReturnType<typeof parseBeliefBooks>
  readonly watchlists: ReturnType<typeof parseWatchlists>
  readonly price_memories: ReturnType<typeof parsePriceMemories>
  readonly history_reads: ReturnType<typeof parseHistoryReads>
  readonly pending_plan_events: ReturnType<typeof parsePendingPlanEvents>
}

const ROOT_KEYS = ["market_memberships", "report_correction_operations", "runtime_state", "setup", "seed", "snapshot", "auction_orders", "resting_orders", "book_next_sequences", "filled_orders", "price_history", "market_minute_closes", "rng_state", "npc_attention", "retail_experience", "parent_orders", "npc_order_lifecycles", "pending_player", "pending_npc", "ingress_receipt_cursors", "next_order_id", "civil_clock", "company_system", "corporate_actions", "public_library", "disclosures", "plans", "urgency_policy", "information_states", "belief_books", "watchlists", "price_memories", "history_reads", "pending_plan_events"] as const

export function parseStrictSaveEnvelope(value: unknown): StrictSaveEnvelope {
  const root = record(value, "根节点")
  exact(root, [...ROOT_KEYS, "retained_market_history"], "根节点")
  const order = parseOrderState(root)
  const setup = parseSetup(root.setup, "setup")
  if (setup.simulation_policy_id !== "a-share-simulation") {
    throw new Error(`存档 setup.simulation_policy_id 必须为 a-share-simulation，实际为 ${JSON.stringify(setup.simulation_policy_id)}`)
  }
  const snapshot = parseSaveSnapshot(root.snapshot, "snapshot")
  const memberships = parseMarketMemberships(root.market_memberships)
  validateMembershipAccounts(memberships, setup, snapshot)
  exact(order.book_next_sequences, setup.stocks.map((stock) => stock.code), "book_next_sequences (setup.stocks)")
  exact(order.book_next_sequences, Object.keys(snapshot.markets), "book_next_sequences (snapshot.markets)")
  exact(root.history_reads as Record<string, unknown>, Object.keys(snapshot.accounts), "history_reads (snapshot.accounts)")
  const runtime = parseSaveRuntime(root.runtime_state)
  const clock = parseCivilClock(root.civil_clock)
  validateCashExReferenceFacts(snapshot, setup.stocks, clock.current_date)
  const retainedHistory = parseRetainedHistory(root.retained_market_history, { ...setup, settled_through: clock.settled_through })
  validateRetainedHistoryCandles(retainedHistory, snapshot.daily_candles)
  for (const [code, bars] of Object.entries(runtime.active_minute_history)) {
    const stock = setup.stocks.find((stock) => stock.code === code)
    if (stock === undefined) throw new SaveSchemaError(`runtime_state.active_minute_history.${code}`, "证券不存在")
    parseMinuteBars(bars, `runtime_state.active_minute_history.${code}`, stock, setup)
  }
  const corrections = parseReportCorrections(root.report_correction_operations)
  const companySystem = parseCompanySystemState(root.company_system)
  const corporateActions = parseSessionCorporateActions(root.corporate_actions, { issuers: companySystem.issuers, setup, snapshot, currentDate: clock.current_date })
  const library = parsePublicLibrary(root.public_library)
  for (const report of library.reports) {
    if (report.source !== "SimpleGenerated" || !Object.hasOwn(companySystem.issuers, report.company)) throw new SaveSchemaError("public_library.reports", "公开报告来源或公司身份与本局 Simple 系统不一致")
  }
  for (const announcement of library.announcements) {
    if (!Object.hasOwn(companySystem.issuers, announcement.company)) throw new SaveSchemaError("public_library.announcements", "公告公司不属于当前发行人集合")
  }
  validateCompanySystemSession(companySystem, setup, clock.current_date)
  validateReportCorrectionLinks(corrections, companySystem, library, clock.current_date)
  for (const [account, confirmations] of Object.entries(runtime.personal_trade_confirmations)) {
    const path = `runtime_state.personal_trade_confirmations.${account}`
    if (!Object.hasOwn(snapshot.accounts, account)) throw new SaveSchemaError(path, "账户不存在")
    for (const [index, confirmation] of confirmations.entries()) {
      if (!Object.hasOwn(snapshot.markets, confirmation.code) || confirmation.civil_date < setup.start_date || confirmation.civil_date > clock.current_date) throw new SaveSchemaError(`${path}[${index}]`, "证券或成交自然日不属于当前会话")
    }
  }
  return { retained_market_history: retainedHistory, market_memberships: memberships, report_correction_operations: corrections, runtime_state: runtime, setup, seed: decimal(root.seed, "seed"), snapshot, ...order, retail_experience: parseRetailExperienceStates(root.retail_experience), civil_clock: clock, company_system: companySystem, corporate_actions: corporateActions, public_library: library, disclosures: parseDisclosureDispatch(root.disclosures), plans: parsePlanBook(root.plans), urgency_policy: parseUrgencyPolicy(root.urgency_policy), information_states: parseInformationStates(root.information_states), belief_books: parseBeliefBooks(root.belief_books), watchlists: parseWatchlists(root.watchlists), price_memories: parsePriceMemories(root.price_memories), history_reads: parseHistoryReads(root.history_reads), pending_plan_events: parsePendingPlanEvents(root.pending_plan_events) }
}
