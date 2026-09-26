import { parseClosingRegistry, parseCompanyOperations, parseDisclosureDispatch, parseOperationsWiring, parsePublicLibrary } from "./company/index.ts"
import { parseSetup, parseSnapshot } from "./market.ts"
import { parseOrderState } from "./orders.ts"
import { parseBeliefBooks } from "./personal/beliefs.ts"
import { parseRetailExperienceStates } from "./personal/experience.ts"
import { parseInformationStates } from "./personal/information.ts"
import { parsePriceMemories, parseWatchlists } from "./personal/memory.ts"
import { parsePendingPlanEvents, parsePlanBook } from "./personal/plans.ts"
import { decimal, exact, integer, record } from "./primitives.ts"
import { parseCivilClock } from "./civil-clock.ts"
import { parseSaveRuntimeV2 } from "./runtime-v2.ts"

export type StrictSaveEnvelope = {
  readonly schema_version: 2
  readonly runtime_v2: ReturnType<typeof parseSaveRuntimeV2>
  readonly setup: ReturnType<typeof parseSetup>
  readonly seed: string
  readonly snapshot: ReturnType<typeof parseSnapshot>
  readonly auction_orders: ReturnType<typeof parseOrderState>["auction_orders"]
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
  readonly information_states: ReturnType<typeof parseInformationStates>
  readonly belief_books: ReturnType<typeof parseBeliefBooks>
  readonly watchlists: ReturnType<typeof parseWatchlists>
  readonly price_memories: ReturnType<typeof parsePriceMemories>
  readonly pending_plan_events: ReturnType<typeof parsePendingPlanEvents>
}

const ROOT_KEYS = ["schema_version", "runtime_v2", "setup", "seed", "snapshot", "auction_orders", "resting_orders", "filled_orders", "price_history", "market_minute_closes", "rng_state", "npc_attention", "retail_experience", "parent_orders", "npc_order_lifecycles", "pending_player", "pending_npc", "next_order_id", "civil_clock", "company_operations", "closing_registry", "public_library", "ops_wiring", "disclosures", "plans", "information_states", "belief_books", "watchlists", "price_memories", "pending_plan_events"] as const

export function parseStrictSaveEnvelope(value: unknown): StrictSaveEnvelope {
  const root = record(value, "根节点")
  exact(root, ROOT_KEYS, "根节点")
  const schemaVersion = integer(root.schema_version, "schema_version", 0)
  if (schemaVersion < 2) throw new Error(`存档 schema_version ${schemaVersion} 是 legacy；仅支持 schema v2`)
  if (schemaVersion > 2) throw new Error(`存档 schema_version ${schemaVersion} newer than supported schema v2`)
  const order = parseOrderState(root)
  return { schema_version: 2, runtime_v2: parseSaveRuntimeV2(root.runtime_v2), setup: parseSetup(root.setup, "setup"), seed: decimal(root.seed, "seed"), snapshot: parseSnapshot(root.snapshot, "snapshot"), ...order, retail_experience: parseRetailExperienceStates(root.retail_experience), civil_clock: parseCivilClock(root.civil_clock), company_operations: parseCompanyOperations(root.company_operations), closing_registry: parseClosingRegistry(root.closing_registry), public_library: parsePublicLibrary(root.public_library), ops_wiring: parseOperationsWiring(root.ops_wiring), disclosures: parseDisclosureDispatch(root.disclosures), plans: parsePlanBook(root.plans), information_states: parseInformationStates(root.information_states), belief_books: parseBeliefBooks(root.belief_books), watchlists: parseWatchlists(root.watchlists), price_memories: parsePriceMemories(root.price_memories), pending_plan_events: parsePendingPlanEvents(root.pending_plan_events) }
}
