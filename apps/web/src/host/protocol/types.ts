import type { AuctionPoint } from "../../types/generated/AuctionPoint.ts";
import type { CivilRefresh } from "../../types/generated/CivilRefresh.ts";
import type { CivilUpdate } from "../../types/generated/CivilUpdate.ts";
import type { ContinuousPoint } from "../../types/generated/ContinuousPoint.ts";
import type { EngineUpdate } from "../../types/generated/EngineUpdate.ts";
import type { Event } from "../../types/generated/Event.ts";
import type { EventFact } from "../../types/generated/EventFact.ts";
import type { PausePreferences } from "../../types/generated/PausePreferences.ts";
import type { Snapshot } from "../../types/generated/Snapshot.ts";
import type { StockSpec } from "../../types/generated/StockSpec.ts";

export type ProtocolCursor = {
  readonly generation: string;
  readonly tick: number;
  readonly seq: number;
};

export type ProtocolState = {
  readonly cursor: ProtocolCursor;
  readonly snapshot: Snapshot;
  readonly securities: readonly StockSpec[];
  readonly publicPublicationIds: readonly string[];
  readonly intraday: readonly NormalizedTickFrame[];
  readonly accepted: ReadonlyMap<string, AcceptedUpdate>;
};

export type AcceptedUpdate = string;

export type NormalizedTickFrame = {
  readonly tick: number;
  readonly events: readonly Event[];
  readonly facts: readonly EventFact[];
  readonly continuousPoints: Readonly<Record<string, ContinuousPoint>>;
  readonly auctionPoints: Readonly<Record<string, readonly AuctionPoint[]>>;
  readonly closedDailyCandles: Snapshot["active_daily_candles"];
  readonly activeDailyCandles: Snapshot["active_daily_candles"];
  readonly markets: Snapshot["markets"];
  readonly seqFrom: number;
  readonly seqTo: number;
};

export type NormalizedTickBatch = {
  readonly kind: "tick-batch";
  readonly frames: readonly NormalizedTickFrame[];
  readonly runtimeSnapshot: Snapshot | null;
};

export type NormalizedCivilUpdate = {
  readonly kind: "civil-update";
  readonly update: CivilUpdate;
  readonly refresh: CivilRefresh;
  readonly intraday: readonly NormalizedTickFrame[];
};

export type NormalizedEngineUpdate = NormalizedTickBatch | NormalizedCivilUpdate;

export type AutomaticOrderPoint = ContinuousPoint & {
  readonly code: string;
};

export type ProtocolEffect =
  | { readonly kind: "notice"; readonly message: string }
  | { readonly kind: "trade"; readonly event: Extract<Event, { Trade: unknown }>["Trade"] }
  | { readonly kind: "automatic-order"; readonly points: readonly AutomaticOrderPoint[] }
  | {
      readonly kind: "civil-barrier";
      readonly barrier: "AfterClose" | "BeforeOpen";
      readonly message: string;
    };

export type ProtocolReduction = {
  readonly kind: "applied";
  readonly state: ProtocolState;
  readonly update: NormalizedEngineUpdate;
  readonly effects: readonly ProtocolEffect[];
} | {
  readonly kind: "exact-retry";
  readonly state: ProtocolState;
  readonly update: NormalizedEngineUpdate;
  readonly effects: readonly ProtocolEffect[];
};

export type ProtocolFailureCode =
  | "PROTOCOL_MALFORMED"
  | "PROTOCOL_CURSOR"
  | "PROTOCOL_REPLAY";

export class ProtocolError extends Error {
  readonly name = "ProtocolError";
  readonly code: ProtocolFailureCode;
  readonly where: string;

  constructor(code: ProtocolFailureCode, where: string, message: string) {
    super(message);
    this.code = code;
    this.where = where;
  }
}

export function createProtocolState(snapshot: Snapshot, generation: string): ProtocolState {
  return {
    cursor: { generation, tick: snapshot.tick, seq: snapshot.seq },
    snapshot,
    securities: [],
    publicPublicationIds: [],
    intraday: [],
    accepted: new Map(),
  };
}

export function defaultPausePreferences(): PausePreferences {
  return { pause_after_close: false, pause_before_open: false };
}

export type { EngineUpdate };
