import assert from "node:assert/strict";
import test from "node:test";
import { LocalStorageSaveRepository } from "./save-repository.ts";

const valid = {
  schema_version: 2,
  seed: "18446744073709551615",
  setup: { stocks: [] },
  snapshot: { seq: 0, tick: 0, markets: {}, accounts: {} },
  auction_orders: {},
  resting_orders: {},
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
    getItem: () => '{"schema_version":1,"seed":18446744073709552000}',
    setItem: () => {},
  });
  assert.throws(() => repository.load(), /seed/);
});

test("local save repository upgrades a legacy safe numeric seed", () => {
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify({ ...valid, seed: 42 }),
    setItem: () => {},
  });
  assert.equal(repository.load()?.seed, "42");
});

test("local save repository preserves a missing schema version as legacy v1 for Rust migration", () => {
  const { schema_version: _schemaVersion, ...legacy } = valid;
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify(legacy),
    setItem: () => {},
  });

  assert.equal(repository.load()?.schema_version, 1);
});

test("local save repository still rejects an explicit unknown schema version", () => {
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify({ ...valid, schema_version: 3 }),
    setItem: () => {},
  });

  assert.throws(() => repository.load(), /不支持的存档 schema_version：3/);
});

test("local save repository migrates a missing legacy stock exchange from a supported code", () => {
  const legacyStock = {
    code: "002156",
    initial_price: 2735,
    category: "MainBoard",
    limit_pct: 0.10,
    v_initial: 2735,
    tick: 1,
    float_shares: 1_000_000,
  };
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify({ ...valid, schema_version: 1, setup: { stocks: [legacyStock] } }),
    setItem: () => {},
  });

  assert.equal(repository.load()?.setup.stocks[0]?.exchange, "Shenzhen");
});

test("local save repository normalizes real v1 stock shapes without preempting Rust rule migration", () => {
  const legacyStocks = [{
    code: "300260",
    initial_price: 3680,
    limit_pct: 0.10,
    v_initial: 3680,
    tick: 1,
    float_shares: 1_000_000,
  }, {
    code: "000812",
    initial_price: 285,
    limit_pct: 0.05,
    v_initial: 285,
    tick: 1,
    float_shares: 1_000_000,
  }];
  const setup = { config: { st_limit: 0.05 }, t1_enabled: false, stocks: legacyStocks };
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify({ ...valid, schema_version: undefined, setup }),
    setItem: () => {},
  });

  const normalized = repository.load();
  assert.equal(normalized?.schema_version, 1);
  assert.equal(normalized?.setup.t1_enabled, false);
  assert.equal(normalized?.setup.config.st_limit, 0.05);
  assert.equal(normalized?.setup.stocks[0]?.category, "ChiNext");
  assert.equal(normalized?.setup.stocks[0]?.limit_pct, 0.10);
  assert.equal(normalized?.setup.stocks[0]?.exchange, "Shenzhen");
  assert.equal(normalized?.setup.stocks[1]?.category, "StMainBoard");
  assert.equal(normalized?.setup.stocks[1]?.limit_pct, 0.05);
  assert.equal(normalized?.setup.stocks[1]?.exchange, "Shenzhen");
});

test("local save repository does not rewrite explicit 5 percent limits in v2", () => {
  const stock = {
    code: "600101",
    exchange: "Shanghai",
    initial_price: 1000,
    category: "StMainBoard",
    limit_pct: 0.05,
    v_initial: 1000,
    tick: 1,
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

test("local save repository rejects a legacy stock whose exchange cannot be inferred", () => {
  const legacyStock = {
    code: "CUSTOM",
    initial_price: 1000,
    category: "MainBoard",
    limit_pct: 0.10,
    v_initial: 1000,
    tick: 1,
    float_shares: 1_000_000,
  };
  const repository = new LocalStorageSaveRepository({
    getItem: () => JSON.stringify({ ...valid, schema_version: 1, setup: { stocks: [legacyStock] } }),
    setItem: () => {},
  });

  assert.throws(() => repository.load(), /无法推断交易所/);
});
