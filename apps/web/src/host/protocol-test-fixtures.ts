import { canonicalJson } from "./protocol/canonical.ts";
import { parseEngineUpdate } from "./protocol/parse.ts";
import { createProtocolState } from "./protocol/types.ts";

export interface JsonRecord {
  readonly [key: string]: JsonValue;
}

export type JsonValue = bigint | boolean | null | number | string | readonly JsonValue[] | JsonRecord | Map<string, JsonValue>;

export type TickBatchWire = {
  readonly frames: readonly JsonRecord[];
  readonly runtime_snapshot: JsonRecord | null;
};

export function dailyCandle(): JsonRecord {
  return { time: 0, open: 1_000, high: 1_000, low: 1_000, close: 1_000, volume: 0 };
}

export function market(): JsonRecord {
  return { last_price: 1_000, last_close: 1_000, best_bid: null, best_ask: null, bids: [], asks: [] };
}

export function snapshot(tick: number, seq: number): JsonRecord {
  return {
    seq,
    tick,
    day: 0,
    phase: "Continuous",
    markets: { "600000": market() },
    accounts: {},
    daily_candles: { "600000": [] },
    active_daily_candles: { "600000": dailyCandle() },
  };
}

export function rejection(seq: number, code = "600000"): JsonRecord {
  return { IntentRejected: { seq, account: 0, code, reason: "UnknownStock" } };
}

export function fact(seq: number, localEventIndex: number, code = "600000"): JsonRecord {
  const event = rejection(seq, code);
  return {
    key: { phase_rank: 4, entity: { Account: 0 }, source: "Sealed", local_event_index: localEventIndex },
    event,
    canonical_payload: canonicalJson(event),
  };
}

export function timeseries(tick: number): JsonRecord {
  return {
    markets: { "600000": market() },
    active_daily_candles: { "600000": dailyCandle() },
    closed_daily_candles: {},
    auction_points: {},
    continuous_points: {
      "600000": { tick, phase: "Continuous", last_price: 1_000, cumulative_volume: tick, bids: [], asks: [] },
    },
  };
}
export function frame(tick: number, seqFrom: number, codes: readonly string[]): JsonRecord {
  return {
    tick,
    events: codes.map((code, index) => rejection(seqFrom + index + 1, code)),
    facts: codes.map((code, index) => fact(seqFrom + index + 1, index, code)),
    timeseries_payload: timeseries(tick),
    seq_from: seqFrom,
    seq_to: seqFrom + codes.length,
  };
}

export function tickBatch(frames: readonly JsonRecord[], runtimeSnapshot: JsonRecord | null): { readonly TickBatch: TickBatchWire } {
  return { TickBatch: { frames, runtime_snapshot: runtimeSnapshot } };
}

export function baseState(): ReturnType<typeof createProtocolState> {
  const initial = parseEngineUpdate(tickBatch([frame(0, 0, [])], snapshot(0, 0)));
  if (!("TickBatch" in initial) || initial.TickBatch.runtime_snapshot === null) {
    throw new Error("test fixture invalid");
  }
  return createProtocolState(initial.TickBatch.runtime_snapshot, "generation-1");
}

export function firstFrame(batch: TickBatchWire): JsonRecord {
  const item = batch.frames[0];
  if (item === undefined) throw new Error("test fixture invalid");
  return item;
}

export function recordArray(value: JsonValue | undefined, name: string): readonly JsonRecord[] {
  if (!Array.isArray(value) || value.some((entry) => !isJsonRecord(entry))) {
    throw new Error(`test fixture invalid: ${name}`);
  }
  return value;
}

export function isJsonRecord(value: JsonValue): value is JsonRecord {
  return typeof value === "object" && value !== null && !Array.isArray(value) && !(value instanceof Map);
}

export function securities(): readonly JsonRecord[] {
  return [{
    code: "600000",
    exchange: "Shanghai",
    initial_price: 1_000,
    category: "MainBoard",
    limit_pct: 0.1,
    tick: 1,
    total_shares: "1000000",
    float_shares: 1_000_000,
  }];
}

export function civilUpdate(): JsonRecord {
  const event: JsonRecord = {
    CivilDateAdvanced: {
      seq: 2,
      settled_date: "2030-01-02",
      next_date: "2030-01-03",
      next_status: "Trading",
    },
  };
  return {
    CivilUpdate: {
      boundary: {
        settled_date: "2030-01-02",
        settled_phase: "IntradayTrading",
        next_date: "2030-01-03",
        next_status: "Trading",
      },
      kinds: ["AfterClose", "BeforeOpen"],
      tick: 1,
      civil_date: "2030-01-03",
      events: [event],
      facts: [{
        key: { phase_rank: 6, entity: "Session", source: "Session", local_event_index: 0 },
        event,
        canonical_payload: canonicalJson(event),
      }],
      seq_from: 1,
      seq_to: 2,
      refresh: {
        ticks_per_day: 1,
        snapshot: snapshot(1, 2),
        securities: securities(),
        intraday: [frame(1, 0, ["600000"])],
        public_publication_ids: [],
      },
    },
  };
}
