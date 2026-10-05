import assert from "node:assert/strict"
import test from "node:test"
import { normalizeSerdeMaps, prepareSaveForWasm } from "./serde-normalize.ts"
import { currentSaveFixture } from "../save/current-save-fixture.ts"

test("WASM normalization 不丢失 OpaqueSubjectId 的对象原型同名键", { timeout: 10000 }, () => {
  const source = new Map([["__proto__", { account_id: "0" }], ["constructor", { account_id: "42" }]])
  const expected = Object.fromEntries(source)
  assert.deepEqual(normalizeSerdeMaps(source), expected)
  assert.deepEqual(normalizeSerdeMaps(expected), expected)
})

test("WASM Restore 的所有 AccountId map 保留 MAX 字符串而 PlanId 仍为 number", { timeout: 10000 }, () => {
  const maximum = "18446744073709551615"
  const slot = currentSaveFixture()
  slot.snapshot = { accounts: { [maximum]: {} } }
  slot.runtime_state = { strategy_states: { [maximum]: {} } }
  for (const field of ["npc_attention", "retail_experience", "parent_orders", "information_states", "belief_books", "watchlists", "price_memories"]) slot[field] = { [maximum]: {} }
  slot.plans = { plans: { "2": {} } }
  const prepared = prepareSaveForWasm(slot as never) as Record<string, unknown>
  assert.deepEqual([...((prepared.snapshot as { accounts: Map<string, unknown> }).accounts).keys()], [maximum])
  assert.deepEqual([...((prepared.runtime_state as { strategy_states: Map<string, unknown> }).strategy_states).keys()], [maximum])
  for (const field of ["npc_attention", "retail_experience", "parent_orders", "information_states", "belief_books", "watchlists", "price_memories"]) assert.deepEqual([...(prepared[field] as Map<string, unknown>).keys()], [maximum])
  assert.deepEqual([...((prepared.plans as { plans: Map<number, unknown> }).plans).keys()], [2])
})
