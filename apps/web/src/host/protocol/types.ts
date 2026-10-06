import type { ContinuousPoint } from "../../types/generated/ContinuousPoint.ts";
import type { EngineUpdate } from "../../types/generated/EngineUpdate.ts";
import type { PausePreferences } from "../../types/generated/PausePreferences.ts";
import type { StockSpec } from "../../types/generated/StockSpec.ts";
import type { PlayerWorkingOrder } from "../player-working-orders.ts";
import type {
  CodedAuctionPoint,
  CodedCivilRefresh,
  CodedCivilUpdate,
  CodedEvent,
  CodedEventFact,
  CodedRuntimeDelta,
  CodedSnapshot,
  CodedTradeEvent,
} from "./stock-identity.ts";

export type ProtocolCursor = {
  readonly generation: string;
  readonly tick: number;
  readonly seq: number;
};

export type ProtocolState = {
  readonly cursor: ProtocolCursor;
  readonly snapshot: CodedSnapshot;
  readonly securities: readonly StockSpec[];
  readonly publicPublicationIds: readonly string[];
  readonly intraday: readonly NormalizedTickFrame[];
  readonly accepted: ReadonlyMap<string, AcceptedUpdate>;
  readonly playerWorkingOrders: Readonly<Record<number, PlayerWorkingOrder>>;
  readonly playerOrdersReady: boolean;
};

export type AcceptedUpdate = string;

export type NormalizedTickFrame = {
  readonly tick: number;
  readonly events: readonly CodedEvent[];
  readonly facts: readonly CodedEventFact[];
  readonly continuousPoints: Readonly<Record<string, ContinuousPoint>>;
  readonly auctionPoints: Readonly<Record<string, readonly CodedAuctionPoint[]>>;
  readonly closedDailyCandles: CodedSnapshot["active_daily_candles"];
  readonly activeDailyCandles: CodedSnapshot["active_daily_candles"];
  readonly markets: CodedSnapshot["markets"];
  readonly seqFrom: number;
  readonly seqTo: number;
};

export type NormalizedTickBatch = {
  readonly kind: "tick-batch";
  readonly frames: readonly NormalizedTickFrame[];
  readonly runtimeSnapshot: CodedSnapshot | null;
  readonly runtimeDelta: CodedRuntimeDelta | null;
};

export type NormalizedCivilUpdate = {
  readonly kind: "civil-update";
  readonly update: CodedCivilUpdate;
  readonly refresh: CodedCivilRefresh;
  readonly intraday: readonly NormalizedTickFrame[];
};

export type NormalizedEngineUpdate = NormalizedTickBatch | NormalizedCivilUpdate;

export type AutomaticOrderPoint = ContinuousPoint & {
  readonly code: string;
};

export type ProtocolEffect =
  | { readonly kind: "notice"; readonly message: string }
  | { readonly kind: "trade"; readonly event: CodedTradeEvent & { readonly tick: number } }
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

export function createProtocolState(snapshot: CodedSnapshot, generation: string): ProtocolState {
  return {
    cursor: { generation, tick: snapshot.tick, seq: snapshot.seq },
    snapshot,
    securities: [],
    publicPublicationIds: [],
    intraday: [],
    accepted: new Map(),
    playerWorkingOrders: {},
    playerOrdersReady: false,
  };
}

export function defaultPausePreferences(): PausePreferences {
  return { pause_after_close: false, pause_before_open: false };
}

export type { EngineUpdate };
