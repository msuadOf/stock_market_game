import assert from "node:assert/strict";
import test from "node:test";
import type { SaveSlot } from "../types/engine.ts";
import { normalizeSerdeMaps, prepareSaveForWasm } from "./serde-normalize.ts";

test("WASM save normalization preserves nested Rust maps as JSON objects", () => {
  const raw = new Map<unknown, unknown>([
    ["snapshot", new Map([
      ["markets", new Map<unknown, unknown>([["600000", new Map([["last_price", 1_000]])]])],
      ["accounts", new Map<unknown, unknown>([[0, new Map([["cash", 100_000]])]])],
    ])],
    ["resting_orders", new Map<unknown, unknown>([["600000", []]])],
    ["price_history", new Map<unknown, unknown>([["600000", [1_000, 1_001]]])],
    ["market_minute_closes", new Map<unknown, unknown>([["600000", [
      { absolute_trading_minute: 0, close: 1_001 },
    ]]])],
  ]);

  const normalized = normalizeSerdeMaps<Record<string, unknown>>(raw);

  assert.deepEqual(normalized, {
    snapshot: {
      markets: { "600000": { last_price: 1_000 } },
      accounts: { "0": { cash: 100_000 } },
    },
    resting_orders: { "600000": [] },
    price_history: { "600000": [1_000, 1_001] },
    market_minute_closes: {
      "600000": [{ absolute_trading_minute: 0, close: 1_001 }],
    },
  });
  assert.doesNotThrow(() => JSON.stringify(normalized));
});

test("WASM restore converts serialized numeric account keys back to numeric Map keys", () => {
  const slot = {
    snapshot: {
      accounts: {
        "0": { cash: 100_000, positions: {} },
        "12": { cash: 200_000, positions: {} },
      },
    },
  };

  const prepared = prepareSaveForWasm(slot as unknown as SaveSlot) as {
    snapshot: { accounts: Map<number, unknown> };
  };

  assert.ok(prepared.snapshot.accounts instanceof Map);
  assert.deepEqual([...prepared.snapshot.accounts.keys()], [0, 12]);
});
