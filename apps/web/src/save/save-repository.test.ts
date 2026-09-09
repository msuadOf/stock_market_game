import assert from "node:assert/strict";
import test from "node:test";
import { LocalStorageSaveRepository } from "./save-repository.ts";

const valid = {
  seed: "18446744073709551615",
  setup: { stocks: [] },
  snapshot: {
    seq: 0,
    tick: 0,
    day: 0,
    phase: "CallAuction",
    markets: {},
    accounts: {},
    daily_candles: {},
    active_daily_candles: {},
  },
  auction_orders: {},
  resting_orders: {},
  price_history: {},
  rng_state: "42",
  npc_attention: {},
  pending_player: [],
  next_order_id: 1,
};

test("local save repository preserves a u64 seed as a decimal string", () => {
  let stored: string | null = null;
  const repository = new LocalStorageSaveRepository({
    getItem: () => stored,
    setItem: (_key, value) => { stored = value; },
  });
  repository.save(valid as never);
  assert.equal(repository.load()?.seed, "18446744073709551615");
});

test("local save repository rejects malformed persisted data", () => {
  const repository = new LocalStorageSaveRepository({
    getItem: () => '{"seed":18446744073709552000}',
    setItem: () => {},
  });
  assert.throws(() => repository.load(), /seed/);
});

test("local save repository rejects a numeric seed instead of migrating it", () => {
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify({ ...valid, seed: 42 }),
    setItem: () => {},
  });
  assert.throws(() => repository.load(), /seed/);
});

test("local save repository rejects obsolete versioned saves", () => {
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify({ ...valid, schema_version: 1 }),
    setItem: () => {},
  });
  assert.throws(() => repository.load(), /不支持旧格式/);
});

test("local save repository requires explicit NPC attention state", () => {
  const { npc_attention: _removed, ...missingAttention } = valid;
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify(missingAttention),
    setItem: () => {},
  });

  assert.throws(() => repository.load(), /注意力/);
});

test("local save repository does not rewrite explicit stock rules", () => {
  const stock = {
    code: "600101",
    exchange: "Shanghai",
    initial_price: 1000,
    category: "StMainBoard",
    limit_pct: 0.05,
    v_initial: 1000,
    tick: 1,
    total_shares: "2000000",
    float_shares: 1_000_000,
  };
  const setup = { config: { st_limit: 0.05 }, stocks: [stock] };
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify({ ...valid, setup }),
    setItem: () => {},
  });

  const current = repository.load();
  assert.equal(current?.setup.config.st_limit, 0.05);
  assert.equal(current?.setup.stocks[0]?.limit_pct, 0.05);
});

test("local save repository requires explicit stock exchange and category", () => {
  const stock = {
    code: "600101",
    initial_price: 1000,
    limit_pct: 0.10,
    v_initial: 1000,
    tick: 1,
    total_shares: "2000000",
    float_shares: 1_000_000,
  };
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify({ ...valid, setup: { stocks: [stock] } }),
    setItem: () => {},
  });

  assert.throws(() => repository.load(), /交易所/);
});

test("local save repository requires lossless total shares", () => {
  const stock = {
    code: "600101",
    exchange: "Shanghai",
    initial_price: 1000,
    category: "MainBoard",
    limit_pct: 0.10,
    v_initial: 1000,
    tick: 1,
    float_shares: 1_000_000,
  };
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify({ ...valid, setup: { stocks: [stock] } }),
    setItem: () => {},
  });

  assert.throws(() => repository.load(), /总股本/);
});
