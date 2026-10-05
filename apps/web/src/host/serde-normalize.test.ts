import assert from "node:assert/strict";
import test from "node:test";
import { publicReportGold } from "../components/company/public-report-fixture.ts";
import {
  normalizePublicReportById,
  normalizePublicReportPage,
  normalizeSerdeMaps,
  prepareSaveForWasm,
} from "./serde-normalize.ts";

const financials = publicReportGold().financials;
Object.assign(financials, { version_kind: "Original", version_supersedes: null });
Object.assign(financials.balance_sheet, { total_assets: "9007199254740993.01", total_liabilities: "0.00", total_equity: "9007199254740993.01", closing_cash: "1.00" });
Object.assign(financials.income.quarter, { net_income: "1.00" });
Object.assign(financials.income.cumulative, { net_income: "1.00", income_tax: "0.00" });
Object.assign(financials.cash_flow, { operating: "1.00", investing: "0.00", financing: "0.00", net_change: "1.00" });

const publicReport = {
  source: "SimulationAccounting",
  id: "9007199254740993",
  company_id: "C-600101",
  period: "2030-03-31",
  kind: "Quarter",
  version_sequence: "9007199254740993",
  supersedes: null,
  approved_date: "2030-04-01",
  approved_second_of_day: 64_800,
  published_date: "2030-04-02",
  published_second_of_day: 64_800,
  financials,
  accounting: {
    total_assets: "9007199254740993.01",
    total_liabilities: "0.00",
    total_equity: "9007199254740993.01",
    closing_cash: "1.00",
    quarter_net_income: "1.00",
    net_income: "1.00",
    income_tax: "0.00",
    operating_cash_flow: "1.00",
    investing_cash_flow: "0.00",
    financing_cash_flow: "0.00",
    net_cash_change: "1.00",
    prior_year_net_income: { Unavailable: { reason: "NoPriorYearHistory" } },
  },
};

test("WASM save normalization preserves nested Rust maps as JSON objects", () => {
  const raw = new Map<unknown, unknown>([
    ["snapshot", new Map([
      ["markets", new Map<unknown, unknown>([["600000", new Map([["last_price", "1000"]])]])],
      ["accounts", new Map<unknown, unknown>([[0, new Map([["cash", "100000"]])]])],
    ])],
    ["resting_orders", new Map<unknown, unknown>([["600000", []]])],
    ["price_history", new Map<unknown, unknown>([["600000", ["1000", "1001"]]])],
    ["market_minute_closes", new Map<unknown, unknown>([["600000", [
      { absolute_trading_minute: 0, close: "1001" },
    ]]])],
  ]);

  const normalized = normalizeSerdeMaps<Record<string, unknown>>(raw);

  assert.deepEqual(normalized, {
    snapshot: {
      markets: { "600000": { last_price: "1000" } },
      accounts: { "0": { cash: "100000" } },
    },
    resting_orders: { "600000": [] },
    price_history: { "600000": ["1000", "1001"] },
    market_minute_closes: {
      "600000": [{ absolute_trading_minute: 0, close: "1001" }],
    },
  });
  assert.doesNotThrow(() => JSON.stringify(normalized));
});

test("WASM save normalization preserves Money strings and converts safe bigint integers", () => {
  const normalized = normalizeSerdeMaps<Record<string, unknown>>({
    snapshot: { seq: 3_680n, markets: new Map([["300260", { best_bid: "3680" }]]) },
  });

  assert.deepEqual(normalized, {
    snapshot: { seq: 3_680, markets: { "300260": { best_bid: "3680" } } },
  });
});

test("WASM save normalization preserves absent optional market values for strict boundary rejection", () => {
  const normalized = normalizeSerdeMaps<Record<string, unknown>>({
    snapshot: { markets: new Map([["300260", { best_bid: undefined }]]) },
  });

  assert.deepEqual(normalized, { snapshot: { markets: { "300260": { best_bid: undefined } } } });
});

test("WASM public report parser preserves opaque IDs and decimal strings", () => {
  const page = normalizePublicReportPage(new Map<string, unknown>([
    ["reports", [new Map(Object.entries(publicReport))]],
    ["next_cursor", publicReport.id],
  ]));

  assert.equal(page.reports[0]?.id, "9007199254740993");
  assert.equal(page.reports[0]?.version_sequence, "9007199254740993");
  assert.equal(page.reports[0]?.accounting.total_assets, "9007199254740993.01");
  assert.equal(page.next_cursor, "9007199254740993");
});

test("WASM public report parser rejects malformed worker payloads", () => {
  assert.throws(
    () => normalizePublicReportById({ ...publicReport, accounting: { ...publicReport.accounting, total_assets: 1 } }),
    /精确十进制字符串/,
  );
});

test("WASM public report parser rejects malformed opaque IDs and fixed-point decimals", () => {
  assert.throws(
    () => normalizePublicReportById({ ...publicReport, id: "1e3" }),
    /无损非负十进制字符串/,
  );
  assert.throws(
    () => normalizePublicReportById({ ...publicReport, accounting: { ...publicReport.accounting, total_assets: "1.2" } }),
    /精确十进制字符串/,
  );
});

test("WASM restore converts canonical account IDs back to string Map keys", () => {
  const slot = {
    snapshot: {
      accounts: {
        "0": { cash: "100000", positions: {} },
        "12": { cash: "200000", positions: {} },
      },
    },
    retail_experience: {
      "12": { stocks: {} },
    },
    npc_attention: {},
    parent_orders: {
      "12": { "600000": { target_qty: 100 } },
    },
    runtime_state: {
      poisoned: false,
      next_receipt_base: "0",
      live_envelopes: [],
      retail_projection_seen: [],
      strategy_states: {
        "12": { Institution: { style: "Balanced" } },
      },
    },
    information_states: {},
    belief_books: {},
    watchlists: {},
    price_memories: {},
    plans: { plans: {} },
  };

  const prepared = prepareSaveForWasm(slot as unknown as {
    snapshot: { accounts: Record<string, unknown> };
    retail_experience: Record<string, unknown>;
    npc_attention: Record<string, unknown>;
    parent_orders: Record<string, unknown>;
    runtime_state: { strategy_states: Record<string, unknown> };
    information_states: Record<string, unknown>;
    belief_books: Record<string, unknown>;
    watchlists: Record<string, unknown>;
    price_memories: Record<string, unknown>;
    plans: { plans: Record<string, unknown> };
  }) as {
    snapshot: { accounts: Map<string, unknown> };
    retail_experience: Map<string, unknown>;
    parent_orders: Map<string, unknown>;
    runtime_state: { strategy_states: Map<string, unknown> };
  };

  assert.ok(prepared.snapshot.accounts instanceof Map);
  assert.deepEqual([...prepared.snapshot.accounts.keys()], ["0", "12"]);
  assert.ok(prepared.retail_experience instanceof Map);
  assert.deepEqual([...prepared.retail_experience.keys()], ["12"]);
  assert.ok(prepared.parent_orders instanceof Map);
  assert.deepEqual([...prepared.parent_orders.keys()], ["12"]);
  assert.ok(prepared.runtime_state.strategy_states instanceof Map);
  assert.deepEqual([...prepared.runtime_state.strategy_states.keys()], ["12"]);
});

test("WASM restore rehydrates every new-save account map without changing decimal map keys", () => {
  const slot = {
    snapshot: { accounts: { "0": {} } },
    npc_attention: { "1": {} },
    runtime_state: {
      poisoned: false,
      next_receipt_base: "3",
      live_envelopes: [{ key: "preserved" }],
      retail_projection_seen: [{ index: "2" }],
      strategy_states: { "1": {} },
    },
    retail_experience: { "1": {} },
    parent_orders: { "1": { "600101": {} } },
    information_states: { "1": {} },
    belief_books: { "1": {} },
    watchlists: { "1": {} },
    price_memories: { "1": {} },
    plans: { plans: { "2": {} } },
  };

  const prepared = prepareSaveForWasm(slot as never) as Record<string, unknown>;

  for (const field of [
    "snapshot", "npc_attention", "runtime_state.strategy_states", "retail_experience", "parent_orders",
    "information_states", "belief_books", "watchlists", "price_memories", "plans",
  ]) {
    const value = field === "snapshot"
      ? (prepared.snapshot as { accounts: unknown }).accounts
      : field === "runtime_state.strategy_states"
        ? ((prepared.runtime_state as { strategy_states: unknown }).strategy_states)
      : field === "plans"
        ? (prepared.plans as { plans: unknown }).plans
      : prepared[field];
    assert.ok(value instanceof Map, `${field} must cross the WASM boundary as a Map`);
  }
  const parentOrders = prepared.parent_orders as Map<string, Record<string, unknown>>;
  const accountPlans = parentOrders.get("1");
  assert.deepEqual(Object.keys(accountPlans ?? {}), ["600101"]);
  assert.deepEqual(
    prepared.runtime_state,
    {
      poisoned: false,
      next_receipt_base: "3",
      live_envelopes: [{ key: "preserved" }],
      retail_projection_seen: [{ index: "2" }],
      strategy_states: new Map([["1", {}]]),
    },
    "non-map runtime_state authority must cross the WASM boundary unchanged",
  );
});

test("WASM restore rejects malformed account-map keys", () => {
  assert.throws(
    () => prepareSaveForWasm({ snapshot: { accounts: { "1e3": {} } } } as never),
    /账户 ID/,
  );
  assert.throws(
    () => prepareSaveForWasm({
      snapshot: { accounts: {} },
      runtime_state: { strategy_states: { "18446744073709551616": {} } },
    } as never),
    /策略状态账户 ID/,
  );
  assert.throws(
    () => prepareSaveForWasm({
      snapshot: { accounts: { "01": {} } },
    } as never),
    /账户 ID/,
  );
  assert.throws(
    () => prepareSaveForWasm({
      snapshot: { accounts: { "1": {}, "01": {} } },
    } as never),
    /账户 ID/,
  );
});
