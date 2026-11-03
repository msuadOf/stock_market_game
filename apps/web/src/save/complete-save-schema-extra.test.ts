import assert from "node:assert/strict"
import test from "node:test"
import { boundarySave, mutateBoundary } from "./complete-save-test-helper.ts"
import { parseSaveSlot } from "./save-schema.ts"

test("strict save boundary rejects malformed nested company and personal-state branches", () => {
  assert.doesNotThrow(() => parseSaveSlot(boundarySave()), "compact fixture 本身必须是当前 schema 的合法正例，否则负例可能因基线错误而空转")
  const save = boundarySave()
  assert.ok(typeof save === "object" && save !== null)
  const operations = Reflect.get(Reflect.get(Reflect.get(save, "company_state"), "LegacyOperations"), "operations")
  assert.ok(typeof operations === "object" && operations !== null)
  const companies = Reflect.get(operations, "companies")
  assert.ok(typeof companies === "object" && companies !== null)
  const company = Object.keys(companies)[0]
  const information = Reflect.get(save, "information_states")
  assert.ok(typeof information === "object" && information !== null)
  const account = Object.keys(information)[0]
  const beliefs = Reflect.get(save, "belief_books")
  assert.ok(typeof beliefs === "object" && beliefs !== null)
  const institution = Object.entries(beliefs as Record<string, Record<string, unknown>>)
    .find(([, book]) => {
      const profile = book.profile
      return typeof profile === "object" && profile !== null && Object.hasOwn(profile, "Institution")
    })?.[0]
  const history = Reflect.get(save, "price_history")
  assert.ok(typeof history === "object" && history !== null)
  const market = Object.keys(history)[0]
  const priceMemories = Reflect.get(save, "price_memories")
  assert.ok(typeof priceMemories === "object" && priceMemories !== null)
  const priceMemoryEntry = Object.entries(priceMemories as Record<string, { stocks?: Record<string, unknown> }>)
    .find(([, memory]) => memory.stocks !== undefined && Object.keys(memory.stocks).length > 0)
  const planBook = Reflect.get(save, "plans")
  assert.ok(typeof planBook === "object" && planBook !== null)
  const plans = Reflect.get(planBook, "plans")
  assert.ok(Array.isArray(plans) && plans.length > 0)
  const plan = 0
  if (company === undefined || account === undefined || institution === undefined) throw new Error("mature save must contain company, personal state, and institution belief")
  if (market === undefined || plan === undefined || priceMemoryEntry === undefined) throw new Error("mature save must contain market, plan, and price-memory state")
  const [priceMemoryAccount, priceMemory] = priceMemoryEntry
  const rememberedMarket = Object.keys(priceMemory.stocks!)[0]
  if (rememberedMarket === undefined) throw new Error("mature save must contain a remembered security")

  const cases: readonly [readonly (string | number)[], unknown, RegExp][] = [
    [["setup", "start_date"], "2030-02-30", /setup\.start_date/],
    [["snapshot", "seq"], "0", /snapshot\.seq/],
    [["resting_orders", market], [{ id: 1 }], /resting_orders/],
    [["price_history", market, 0], 1.5, /price_history/],
    [["market_minute_closes", market], [{ absolute_trading_minute: 1, close: 1000 }], /market_minute_closes/],
    [["rng_state"], 42, /rng_state/],
    [["npc_attention", account, "rng_state"], 42, /npc_attention/],
    [["runtime_state", "strategy_states", account], { Institution: "Unknown" }, /runtime_state/],
    [["retail_experience", "1", "consecutive_failed_buys"], "1", /retail_experience/],
    // 旧字段（架构项 8 已删除的母单镜像与待应用事实队列）作为未知键必须被拒绝。
    [["parent_orders"], { [account]: { [market]: { code: market } } }, /parent_orders/],
    [["plans", "plans", plan, "Investor", "execution"], null, /plans\.plans\[\d+\]\.Investor\.execution/],
    [["plans", "plans", plan, "Investor", "execution"], undefined, /plans\.plans\[\d+\]\.Investor\.execution/],
    [["plans", "plans", plan, "Investor", "execution", "limit_price"], 1000, /execution\.limit_price/],
    [["plans", "plans", plan, "Investor", "execution", "expires_market_minute"], "500", /execution\.expires_market_minute/],
    [["npc_order_lifecycles"], [{ account: 1 }], /npc_order_lifecycles/],
    [["pending_player"], [[1, { Retired: {} }]], /pending_player/],
    [["next_order_id"], 9_007_199_254_740_992, /next_order_id/],
    [["civil_clock", "policy", "unexpected"], true, /civil_clock\.policy\.unexpected/],
    [["company_state", "LegacyOperations", "operations", "companies", company, "spec", "unexpected"], true, /company_state\.LegacyOperations\.operations/],
    [["company_state", "LegacyOperations", "closing", "versions"], [{}], /company_state\.LegacyOperations\.closing/],
    [["public_library", "next_seq"], "35", /public_library/],
    [["ops_wiring"], { mirrored: [] }, /ops_wiring/],
    [["civil_clock", "pending_due"], [], /civil_clock\.pending_due/],
    [["civil_clock", "next_due_seq"], 1, /civil_clock\.next_due_seq/],
    [["disclosures", "announced_through"], "2030-02-30", /disclosures/],
    [["plans", "plans", plan, "Investor", "status"], { Unknown: {} }, /plans/],
    [["plans", "plans", plan, "Investor", "review", "last_review_resources"], undefined, /last_review_resources/],
    [["belief_books", account, "experience"], undefined, /belief_books.*experience/],
    [["belief_books", institution, "institution_policy"], undefined, /belief_books.*institution_policy/],
    [["belief_books", institution, "institution_account_risk_paused"], undefined, /institution_account_risk_paused/],
    [["information_states", account, "unexpected"], true, /information_states/],
    [["belief_books", account, "analysis", "fundamental_bp"], "4000", /belief_books/],
    [["watchlists", account, "latest_attention_minute"], 50, /watchlists/],
    [["price_memories", priceMemoryAccount, "stocks", rememberedMarket, "last_touched_minute"], 50, /price_memories/],
    // 旧待应用事实队列字段（架构项 8 已删除）同样必须被拒绝。
    [["pending_plan_events"], [{ Rejected: {} }], /pending_plan_events/],
  ]
  for (const [path, value, expected] of cases) assert.throws(() => parseSaveSlot(mutateBoundary(path, value)), expected)
})
