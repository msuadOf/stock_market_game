import { parseClosingRegistry, parseCompanyOperations, parseDisclosureDispatch, parsePublicLibrary } from "./company/index.ts"
import { parseGroups } from "./company/groups.ts"
import { parseSetup } from "./market.ts"
import { parseSaveSnapshot } from "./save-snapshot.ts"
import { parseOrderState } from "./orders.ts"
import { parseBeliefBooks } from "./personal/beliefs.ts"
import { parseRetailExperienceStates } from "./personal/experience.ts"
import { parseInformationStates } from "./personal/information.ts"
import { parseHistoryReads, parsePriceMemories, parseWatchlists } from "./personal/memory.ts"
import { parsePlanBook } from "./personal/plans.ts"
import { decimal, exact, record } from "./primitives.ts"
import { parseCivilClock } from "./civil-clock.ts"
import { parseMarketMemberships, validateMembershipAccounts } from "./market-memberships.ts"
import { parseSaveRuntime } from "./runtime-state.ts"
import { parseUrgencyPolicy } from "./urgency-policy.ts"

export type StrictSaveEnvelope = {
  readonly runtime_state: ReturnType<typeof parseSaveRuntime>
  readonly setup: ReturnType<typeof parseSetup>
  readonly seed: string
  readonly snapshot: ReturnType<typeof parseSaveSnapshot>
  readonly book_next_sequences: ReturnType<typeof parseOrderState>["book_next_sequences"]
  readonly resting_orders: ReturnType<typeof parseOrderState>["resting_orders"]
  readonly price_history: ReturnType<typeof parseOrderState>["price_history"]
  readonly market_minute_closes: ReturnType<typeof parseOrderState>["market_minute_closes"]
  readonly rng_state: string
  readonly npc_attention: ReturnType<typeof parseOrderState>["npc_attention"]
  readonly retail_experience: ReturnType<typeof parseRetailExperienceStates>
  readonly npc_order_lifecycles: ReturnType<typeof parseOrderState>["npc_order_lifecycles"]
  readonly pending_player: ReturnType<typeof parseOrderState>["pending_player"]
  readonly ingress_receipt_cursors: ReturnType<typeof parseOrderState>["ingress_receipt_cursors"]
  readonly next_order_id: number
  readonly civil_clock: ReturnType<typeof parseCivilClock>
  readonly groups: ReturnType<typeof parseGroups>
  readonly company_operations: ReturnType<typeof parseCompanyOperations>
  readonly closing_registry: ReturnType<typeof parseClosingRegistry>
  readonly public_library: ReturnType<typeof parsePublicLibrary>
  readonly disclosures: ReturnType<typeof parseDisclosureDispatch>
  readonly plans: ReturnType<typeof parsePlanBook>
  readonly urgency_policy: ReturnType<typeof parseUrgencyPolicy>
  readonly information_states: ReturnType<typeof parseInformationStates>
  readonly belief_books: ReturnType<typeof parseBeliefBooks>
  readonly watchlists: ReturnType<typeof parseWatchlists>
  readonly price_memories: ReturnType<typeof parsePriceMemories>
  readonly history_reads: ReturnType<typeof parseHistoryReads>
  /** B3 远程登录：市场成员关系（引擎 SaveSlot.market_memberships 镜像）。 */
  readonly market_memberships: ReturnType<typeof parseMarketMemberships>
}

const ROOT_KEYS = ["runtime_state", "setup", "seed", "snapshot", "resting_orders", "book_next_sequences", "filled_orders", "price_history", "market_minute_closes", "rng_state", "npc_attention", "retail_experience", "npc_order_lifecycles", "pending_player", "ingress_receipt_cursors", "next_order_id", "civil_clock", "groups", "company_operations", "closing_registry", "public_library", "disclosures", "plans", "urgency_policy", "information_states", "belief_books", "watchlists", "price_memories", "history_reads", "market_memberships"] as const

export function parseStrictSaveEnvelope(value: unknown): StrictSaveEnvelope {
  const root = record(value, "根节点")
  // T3 批四（引擎 ADR-0042 §9.6 选项甲）：跨 tick `pending_npc` 载体删除——
  // 携带该键的存档是旧格式，显式拒绝（镜像引擎解码边界 fault2100；不静默
  // 丢键）。根节点 exact 检查本会拒绝未知键，此处先给出精确错误信息。
  if ("pending_npc" in root) {
    throw new Error("旧格式存档（含 pending_npc 键）不被当前版本接受：请用当前引擎重新生成存档")
  }
  exact(root, ROOT_KEYS, "根节点")
  const order = parseOrderState(root)
  const setup = parseSetup(root.setup, "setup")
  if (setup.simulation_policy_id !== "a-share-simulation") {
    throw new Error(`存档 setup.simulation_policy_id 必须为 a-share-simulation，实际为 ${JSON.stringify(setup.simulation_policy_id)}`)
  }
  const snapshot = parseSaveSnapshot(root.snapshot, "snapshot")
  // ADR 0041：引擎按代码升序为证券分配 StockId（0..n），存档 wire 的股票 map 键是
  // id 的字符串形态。cross-check 用存档自带 setup 推导期望 id 键集合（代码串不出现在键位）。
  const expectedStockIdKeys = [...setup.stocks.map((stock) => stock.code)].sort().map((_, index) => String(index))
  exact(order.book_next_sequences, expectedStockIdKeys, "book_next_sequences (setup.stocks→StockId)")
  exact(order.book_next_sequences, Object.keys(snapshot.markets), "book_next_sequences (snapshot.markets)")
  // B3 远程登录：成员关系 schema 校验+账户集合恰好校验（与引擎 fault2020
  // 域同语义——web 严格 parser 不放宽）。
  const market_memberships = parseMarketMemberships(root.market_memberships)
  validateMembershipAccounts(market_memberships, setup, snapshot)
  const runtime_state = parseSaveRuntime(root.runtime_state)
  exact(runtime_state.book_public_cutoff_sequences, expectedStockIdKeys, "runtime_state.book_public_cutoff_sequences (setup.stocks→StockId)")
  for (const [code, cutoff] of Object.entries(runtime_state.book_public_cutoff_sequences)) {
    if (BigInt(cutoff) > BigInt(order.book_next_sequences[code])) {
      throw new Error(`存档 runtime_state.book_public_cutoff_sequences.${code} 超过 book_next_sequences`)
    }
  }
  exact(root.history_reads as Record<string, unknown>, Object.keys(snapshot.accounts), "history_reads (snapshot.accounts)")
  return { runtime_state, setup, seed: decimal(root.seed, "seed"), snapshot, ...order, retail_experience: parseRetailExperienceStates(root.retail_experience), civil_clock: parseCivilClock(root.civil_clock), groups: parseGroups(root.groups), company_operations: parseCompanyOperations(root.company_operations), closing_registry: parseClosingRegistry(root.closing_registry), public_library: parsePublicLibrary(root.public_library), disclosures: parseDisclosureDispatch(root.disclosures), plans: parsePlanBook(root.plans), urgency_policy: parseUrgencyPolicy(root.urgency_policy), information_states: parseInformationStates(root.information_states), belief_books: parseBeliefBooks(root.belief_books), watchlists: parseWatchlists(root.watchlists), price_memories: parsePriceMemories(root.price_memories), history_reads: parseHistoryReads(root.history_reads), market_memberships }
}
