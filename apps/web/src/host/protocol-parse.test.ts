import assert from "node:assert/strict";
import test from "node:test";
import { canonicalJson } from "./protocol/canonical.ts";
import { parseNormalizedEngineUpdate } from "./protocol/index.ts";
import { parseEngineUpdate } from "./protocol/parse.ts";
import {
  civilUpdate,
  dailyCandle,
  firstFrame,
  frame,
  market,
  snapshot,
  tickBatch,
  timeseries,
  isJsonRecord,
} from "./protocol-test-fixtures.ts";
import type { JsonRecord, JsonValue } from "./protocol-test-fixtures.ts";

function civilRefreshWithStock(stock: JsonRecord): JsonRecord {
  const raw = civilUpdate();
  const update = raw["CivilUpdate"];
  if (!isJsonRecord(update)) {
    throw new Error("test fixture invalid");
  }
  const refresh = update["refresh"];
  if (!isJsonRecord(refresh)) {
    throw new Error("test fixture invalid");
  }
  return { CivilUpdate: { ...update, refresh: { ...refresh, securities: [stock] } } };
}

test("Given WASM Map fields, when parsed, then nested maps normalize at the boundary", () => {
  const raw = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));
  const current = firstFrame(raw.TickBatch);
  const withMaps = {
    TickBatch: {
      ...raw.TickBatch,
      frames: [{
        ...current,
        timeseries_payload: new Map([
          ["markets", new Map([["600000", market()]])],
          ["active_daily_candles", new Map([["600000", dailyCandle()]])],
          ["closed_daily_candles", new Map()],
          ["auction_points", new Map()],
          ["continuous_points", new Map([["600000", { tick: 1n, phase: "Continuous", last_price: 1_000n, cumulative_volume: 1n, bids: [], asks: [] }]])],
        ]),
      }],
    },
  };

  const normalized = parseNormalizedEngineUpdate(withMaps);

  assert.equal(normalized.kind, "tick-batch");
  assert.equal(normalized.frames[0]?.continuousPoints["600000"]?.tick, 1);
});

test("Given every Event wire variant, when parsed in an update, then all twelve tags are accepted", () => {
  const events: readonly JsonRecord[] = [
    { Trade: { seq: 1, code: "600000", price: 1_000, qty: 100, maker: 0, taker: 1 } },
    { AuctionTick: { seq: 2, tick: 1, phase: "CallAuction", code: "600000", indicative_price: null, matched_volume: 0, imbalance: 0 } },
    { AuctionCompleted: { seq: 3, tick: 1, phase: "ClosingAuction", code: "600000", clearing_price: 1_000, matched_volume: 100 } },
    { PriceTick: { seq: 4, tick: 1, code: "600000", last_price: 1_000, daily_candle: dailyCandle(), bids: [], asks: [] } },
    { DayBoundary: { seq: 5, day: 1, closed_daily_candles: { "600000": dailyCandle() } } },
    { CivilDateAdvanced: { seq: 6, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: { Closed: { OfficialHoliday: { citation_id: "SSE-2030" } } } } },
    { CompanyDisclosurePublished: { seq: 7, publication_id: 7, company: "C-600000", published_at: { date: "2030-01-03", second_of_day: 64_800 }, kind: { Report: { report_revision: 1 } } } },
    { IntentRejected: { seq: 8, account: 0, code: "600000", reason: "SameTickOrderNotCancelable" } },
    { SettlementError: { seq: 9, account: 0, code: "600000", reason: "settlement failed" } },
    { ResourceLimit: { seq: 10, resource: "PendingPlanEvents", limit: 99 } },
    { OrderCanceled: { seq: 11, account: 0, code: "600000", id: 8, remaining_qty: 100 } },
    { OrderAccepted: { seq: 12, account: 0, code: "600000", id: 9, side: "Buy", price: 1_000, remaining_qty: 100 } },
  ];
  const keys: readonly JsonRecord[] = [
    { phase_rank: 4, entity: { Stock: "600000" }, source: "Sealed", local_event_index: 0 },
    { phase_rank: 4, entity: { Stock: "600000" }, source: "PriceTick", local_event_index: 0 },
    { phase_rank: 4, entity: { Stock: "600000" }, source: "PriceTick", local_event_index: 1 },
    { phase_rank: 4, entity: { Stock: "600000" }, source: "PriceTick", local_event_index: 2 },
    { phase_rank: 5, entity: "Session", source: "DayEnd", local_event_index: 0 },
    { phase_rank: 6, entity: "Session", source: "Session", local_event_index: 0 },
    { phase_rank: 6, entity: "Session", source: "Session", local_event_index: 1 },
    { phase_rank: 4, entity: { Account: 0 }, source: "Sealed", local_event_index: 0 },
    { phase_rank: 4, entity: { Account: 0 }, source: "Sealed", local_event_index: 1 },
    { phase_rank: 6, entity: "Session", source: "Session", local_event_index: 2 },
    { phase_rank: 4, entity: { Account: 0 }, source: "Sealed", local_event_index: 2 },
    { phase_rank: 4, entity: { Account: 0 }, source: "Sealed", local_event_index: 3 },
  ];
  const update = {
    TickBatch: {
      frames: [{
        tick: 1,
        events,
        facts: events.map((event, index) => ({ key: keys[index] ?? {}, event, canonical_payload: canonicalJson(event) })),
        timeseries_payload: timeseries(1),
        seq_from: 0,
        seq_to: 12,
      }],
      runtime_snapshot: snapshot(1, 12),
    },
  };

  const parsed = parseEngineUpdate(update);

  assert.equal("TickBatch" in parsed && parsed.TickBatch.frames[0]?.events.length, 12);
});

test("Given malformed tags, fields, keys, bigint, or enums, when parsed, then each is rejected", () => {
  const valid = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));
  const current = firstFrame(valid.TickBatch);
  const malformedInputs = [
    ["extra", tickBatch([{ ...current, extra: true }], snapshot(1, 1))],
    ["unknown", { Unknown: valid.TickBatch }],
    ["badTag", tickBatch([{ ...current, events: [{ Wrong: { seq: 1 } }] }], snapshot(1, 1))],
    ["unsafe", tickBatch([{ ...current, tick: 9_007_199_254_740_992n }], snapshot(1, 1))],
  ] as const;

  for (const [name, malformed] of malformedInputs) {
    assert.throws(() => parseEngineUpdate(malformed), { code: "PROTOCOL_MALFORMED" }, name);
  }
});

test("Given a decimal string beyond u64, when parsed, then opaque protocol IDs and totals are rejected", () => {
  const valid = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));
  const current = firstFrame(valid.TickBatch);
  const oversizedStock = {
    CivilUpdate: {
      boundary: { settled_date: "2030-01-02", settled_phase: "ClosedDay", next_date: "2030-01-03", next_status: "Trading" },
      kinds: ["BeforeOpen"],
      tick: 0,
      civil_date: "2030-01-03",
      events: [{ CivilDateAdvanced: { seq: 1, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: "Trading" } }],
      facts: [{
        key: { phase_rank: 6, entity: "Session", source: "Session", local_event_index: 0 },
        event: { CivilDateAdvanced: { seq: 1, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: "Trading" } },
        canonical_payload: canonicalJson({ CivilDateAdvanced: { seq: 1, settled_date: "2030-01-02", next_date: "2030-01-03", next_status: "Trading" } }),
      }],
      seq_from: 0,
      seq_to: 1,
      refresh: {
        ticks_per_day: 1,
        snapshot: snapshot(0, 1),
        securities: [{ code: "600000", exchange: "Shanghai", initial_price: 1_000, category: "MainBoard", limit_pct: 0.1, tick: 1, total_shares: "18446744073709551616", float_shares: 0 }],
        intraday: [],
        public_publication_ids: [],
      },
    },
  };

  assert.throws(() => parseEngineUpdate(oversizedStock), { code: "PROTOCOL_MALFORMED" });
  assert.throws(() => parseEngineUpdate({
    TickBatch: {
      ...valid.TickBatch,
      frames: [{
        ...current,
        timeseries_payload: {
          ...timeseries(1),
          active_daily_candles: { "600000": { ...dailyCandle(), trade_stats: { turnover_cents: "18446744073709551616", trade_count: 0 } } },
        },
      }],
    },
  }), { code: "PROTOCOL_MALFORMED" });
});

test("Given malformed StockSpec structural fields, when parsed, then transport rejects only those fields", () => {
  const valid = {
    code: "600000",
    exchange: "Shanghai",
    initial_price: 1_000,
    category: "MainBoard",
    limit_pct: 0.1,
    tick: 1,
    total_shares: "1000000",
    float_shares: 1_000_000,
  };
  const malformedStocks = [
    { ...valid, exchange: "FutureExchange" },
    { ...valid, category: "FutureBoard" },
    { ...valid, total_shares: "not-a-u64" },
    { ...valid, float_shares: 4_294_967_296 },
  ];

  for (const stock of malformedStocks) {
    assert.throws(() => parseEngineUpdate(civilRefreshWithStock(stock)), { code: "PROTOCOL_MALFORMED" });
  }
});

test("Given structural-key attacks or normalized Map collisions, when parsed, then the boundary rejects them", () => {
  const valid = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));
  const current = firstFrame(valid.TickBatch);
  const parsedJson: unknown = JSON.parse('{"TickBatch":{"frames":[{"tick":1,"events":[],"facts":[],"timeseries_payload":{"markets":{"__proto__":{"last_price":1000}},"active_daily_candles":{},"closed_daily_candles":{},"auction_points":{},"continuous_points":{}},"seq_from":0,"seq_to":0}],"runtime_snapshot":null}}');
  const collidingMaps = {
    TickBatch: {
      ...valid.TickBatch,
      frames: [{
        ...current,
        timeseries_payload: new Map([
          ["markets", new Map<unknown, JsonValue>([[1, market()], ["1", market()]])],
          ["active_daily_candles", new Map()],
          ["closed_daily_candles", new Map()],
          ["auction_points", new Map()],
          ["continuous_points", new Map()],
        ]),
      }],
    },
  };

  for (const forbidden of ["__proto__", "prototype", "constructor"]) {
    const dangerous: unknown = JSON.parse(`{"TickBatch":{"frames":[{"tick":1,"events":[],"facts":[],"timeseries_payload":{"markets":{"${forbidden}":{}},"active_daily_candles":{},"closed_daily_candles":{},"auction_points":{},"continuous_points":{}},"seq_from":0,"seq_to":0}],"runtime_snapshot":null}}`);
    assert.throws(() => parseEngineUpdate(dangerous), { code: "PROTOCOL_MALFORMED" }, forbidden);
  }
  assert.throws(() => parseEngineUpdate(parsedJson), { code: "PROTOCOL_MALFORMED" });
  assert.throws(() => parseEngineUpdate(collidingMaps), { code: "PROTOCOL_MALFORMED" });
});

test("Given Map keys that normalize to the same string, when normalized, then their distinct raw identities are rejected", () => {
  const raw = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));
  const current = firstFrame(raw.TickBatch);
  const colliding = {
    TickBatch: {
      ...raw.TickBatch,
      frames: [{
        ...current,
        timeseries_payload: new Map([
          ["markets", new Map<unknown, JsonValue>([[1, market()], ["1", market()]])],
          ["active_daily_candles", new Map()],
          ["closed_daily_candles", new Map()],
          ["auction_points", new Map()],
          ["continuous_points", new Map()],
        ]),
      }],
    },
  };

  assert.throws(() => parseEngineUpdate(colliding), { code: "PROTOCOL_MALFORMED" });
});

test("Given a safe WASM Map, when normalized, then parsed records have no prototype", () => {
  const raw = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));
  const current = firstFrame(raw.TickBatch);
  const parsed = parseEngineUpdate({
    TickBatch: {
      ...raw.TickBatch,
      frames: [{
        ...current,
        timeseries_payload: new Map([
          ["markets", new Map([["600000", market()]])],
          ["active_daily_candles", new Map([["600000", dailyCandle()]])],
          ["closed_daily_candles", new Map()],
          ["auction_points", new Map()],
          ["continuous_points", new Map()],
        ]),
      }],
    },
  });

  if (!("TickBatch" in parsed)) throw new Error("test fixture invalid");
  assert.equal(Object.getPrototypeOf(parsed.TickBatch.frames[0]?.timeseries_payload.markets), null);
  assert.deepEqual(parsed.TickBatch.frames[0]?.timeseries_payload.markets["600000"], market());
});

test("Given Rust-width boundary values, when parsed, then u8 and u32 fields keep their actual widths", () => {
  const valid = tickBatch([frame(1, 0, ["600000"])], snapshot(1, 1));
  const current = firstFrame(valid.TickBatch);
  const disclosure = {
    CompanyDisclosurePublished: {
      seq: 1,
      publication_id: 1,
      company: "C-future",
      published_at: { date: "2030-01-03", second_of_day: 0 },
      kind: { Report: { report_revision: 4_294_967_295 } },
    },
  };
  const fact = {
    key: { phase_rank: 255, entity: "Session", source: "Session", local_event_index: 0 },
    event: disclosure,
    canonical_payload: canonicalJson(disclosure),
  };
  const acceptedFrame = { ...current, events: [disclosure], facts: [fact], seq_from: 0, seq_to: 1 };
  const accepted = {
    TickBatch: {
      frames: [acceptedFrame],
      runtime_snapshot: snapshot(1, 1),
    },
  };

  assert.doesNotThrow(() => parseEngineUpdate(accepted));
  assert.throws(() => parseEngineUpdate({
    TickBatch: { ...accepted.TickBatch, frames: [{ ...acceptedFrame, facts: [{ ...fact, key: { ...fact.key, phase_rank: 256 } }] }] },
  }), { code: "PROTOCOL_MALFORMED" });
  assert.throws(() => parseEngineUpdate({
    TickBatch: { ...accepted.TickBatch, frames: [{ ...acceptedFrame, events: [{ CompanyDisclosurePublished: { ...disclosure.CompanyDisclosurePublished, kind: { Report: { report_revision: 4_294_967_296 } } } }] }] },
  }), { code: "PROTOCOL_MALFORMED" });
  assert.throws(() => parseEngineUpdate({
    TickBatch: { ...accepted.TickBatch, frames: [{ ...acceptedFrame, facts: [{ ...fact, key: { ...fact.key, phase_rank: -1 } }] }] },
  }), { code: "PROTOCOL_MALFORMED" });
});

test("Given a future structurally valid security, when parsed, then transport does not duplicate engine market policy", () => {
  const future = {
    code: "688001",
    exchange: "Shanghai",
    initial_price: 0,
    category: "MainBoard",
    limit_pct: 0.35,
    tick: 2,
    total_shares: "0",
    float_shares: 0,
  };

  assert.doesNotThrow(() => parseEngineUpdate(civilRefreshWithStock(future)));
});
