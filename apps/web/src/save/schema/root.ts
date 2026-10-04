import { parseClosingRegistry, parseCompanyOperations, parseDisclosureDispatch, parseOperationsWiring, parsePublicLibrary } from "./company/index.ts"
import { parseSetup } from "./market.ts"
import { parseSaveSnapshot } from "./save-snapshot.ts"
import { parseOrderState } from "./orders.ts"
import { parseBeliefBooks } from "./personal/beliefs.ts"
import { parseRetailExperienceStates } from "./personal/experience.ts"
import { parseInformationStates } from "./personal/information.ts"
import { parsePriceMemories, parseWatchlists } from "./personal/memory.ts"
import { parsePendingPlanEvents, parsePlanBook } from "./personal/plans.ts"
import { decimal, exact, record } from "./primitives.ts"
import { parseCivilClock } from "./civil-clock.ts"
import { parseSaveRuntime } from "./runtime-state.ts"
import { parseUrgencyPolicy } from "./urgency-policy.ts"

export type StrictSaveEnvelope = {
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
  readonly next_order_id: number
  readonly civil_clock: ReturnType<typeof parseCivilClock>
  readonly company_operations: ReturnType<typeof parseCompanyOperations>
  readonly closing_registry: ReturnType<typeof parseClosingRegistry>
  readonly public_library: ReturnType<typeof parsePublicLibrary>
  readonly ops_wiring: ReturnType<typeof parseOperationsWiring>
  readonly disclosures: ReturnType<typeof parseDisclosureDispatch>
  readonly plans: ReturnType<typeof parsePlanBook>
  readonly urgency_policy: ReturnType<typeof parseUrgencyPolicy>
  readonly information_states: ReturnType<typeof parseInformationStates>
  readonly belief_books: ReturnType<typeof parseBeliefBooks>
  readonly watchlists: ReturnType<typeof parseWatchlists>
  readonly price_memories: ReturnType<typeof parsePriceMemories>
  readonly pending_plan_events: ReturnType<typeof parsePendingPlanEvents>
}

const ROOT_KEYS = ["runtime_state", "setup", "seed", "snapshot", "auction_orders", "resting_orders", "book_next_sequences", "filled_orders", "price_history", "market_minute_closes", "rng_state", "npc_attention", "retail_experience", "parent_orders", "npc_order_lifecycles", "pending_player", "pending_npc", "next_order_id", "civil_clock", "company_operations", "closing_registry", "public_library", "ops_wiring", "disclosures", "plans", "urgency_policy", "information_states", "belief_books", "watchlists", "price_memories", "pending_plan_events"] as const

export function parseStrictSaveEnvelope(value: unknown): StrictSaveEnvelope {
  const root = record(value, "根节点")
  exact(root, ROOT_KEYS, "根节点")
  const order = parseOrderState(root)
  const setup = parseSetup(root.setup, "setup")
  if (setup.simulation_policy_id !== "a-share-simulation") {
    throw new Error(`存档 setup.simulation_policy_id 必须为 a-share-simulation，实际为 ${JSON.stringify(setup.simulation_policy_id)}`)
  }
  const snapshot = parseSaveSnapshot(root.snapshot, "snapshot")
  exact(order.book_next_sequences, setup.stocks.map((stock) => stock.code), "book_next_sequences (setup.stocks)")
  exact(order.book_next_sequences, Object.keys(snapshot.markets), "book_next_sequences (snapshot.markets)")
  return { runtime_state: parseSaveRuntime(root.runtime_state), setup, seed: decimal(root.seed, "seed"), snapshot, ...order, retail_experience: parseRetailExperienceStates(root.retail_experience), civil_clock: parseCivilClock(root.civil_clock), company_operations: parseCompanyOperations(root.company_operations), closing_registry: parseClosingRegistry(root.closing_registry), public_library: parsePublicLibrary(root.public_library), ops_wiring: parseOperationsWiring(root.ops_wiring), disclosures: parseDisclosureDispatch(root.disclosures), plans: parsePlanBook(root.plans), urgency_policy: parseUrgencyPolicy(root.urgency_policy), information_states: parseInformationStates(root.information_states), belief_books: parseBeliefBooks(root.belief_books), watchlists: parseWatchlists(root.watchlists), price_memories: parsePriceMemories(root.price_memories), pending_plan_events: parsePendingPlanEvents(root.pending_plan_events) }
}
