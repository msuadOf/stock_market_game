import assert from "node:assert/strict";
import test from "node:test";
import { normalizeSerdeMaps, prepareSaveForWasm } from "./serde-normalize.ts";

test("Given a WASM save Map, when crossing the host save boundary, then its market keys survive JSON persistence", () => {
  const saved = normalizeSerdeMaps({
    snapshot: new Map([["markets", new Map([["600101", { last_price: 1_120 }]])]]),
  });

  const encoded = JSON.stringify(saved);

  assert.match(encoded, /600101/);
  assert.match(encoded, /last_price/);
});

test("Given a persisted decimal account map, when rehydrated for WASM, then numeric Map keys are restored", () => {
  const prepared = prepareSaveForWasm({
    snapshot: { accounts: { "0": {} } },
    npc_attention: {},
    retail_experience: {},
    parent_orders: {},
    runtime_v2: {
      poisoned: false,
      next_receipt_base: "0",
      live_envelopes: [],
      retail_projection_seen: [],
      strategy_states: {},
    },
    information_states: {},
    belief_books: {},
    watchlists: {},
    price_memories: {},
    plans: { plans: {} },
  }) as { readonly snapshot: { readonly accounts: Map<number, unknown> } }

  assert.deepEqual([...prepared.snapshot.accounts.keys()], [0])
});
