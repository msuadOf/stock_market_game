import {
  createProtocolState,
  ProtocolError,
  reduceEngineUpdate,
  type ProtocolReduction,
  type ProtocolState,
  type ProtocolCursor,
} from "./protocol/index.ts";
import type { HostFailure, HostUpdate } from "./host-update.ts";
import { applyProtocolRuntimeDelta, installProtocolSnapshotBaseline, installProtocolWorkingOrdersBaseline, store } from "../store/store.ts";
import { parseBaselineWorkingOrders } from "./protocol/runtime-delta.ts";

function knownProtocolContext(update: Extract<HostUpdate, { type: "protocol" }>, cursor: ProtocolCursor | null): unknown {
  const actual: Record<string, unknown> = { generation: update.generation };
  const raw = update.update;
  if (raw !== null && typeof raw === "object" && !Array.isArray(raw)) {
    const source = raw as Record<string, unknown>;
    const kinds = (["TickBatch", "CivilUpdate"] as const).filter((kind) => Object.hasOwn(source, kind));
    for (const kind of kinds.length === 1 ? kinds : []) {
      const payload = source[kind];
      if (payload === null || typeof payload !== "object" || Array.isArray(payload)) continue;
      actual.kind = kind;
      const record = payload as Record<string, unknown>;
      const frames = record.frames;
      const first = kind === "CivilUpdate" ? record : Array.isArray(frames) ? frames[0] : undefined;
      const last = kind === "CivilUpdate" ? record : Array.isArray(frames) ? frames.at(-1) : undefined;
      for (const [entry, fields] of [[first, [["tick", "tickFrom"], ["seq_from", "seqFrom"]]], [last, [["tick", "tickTo"], ["seq_to", "seqTo"]]]] as const) {
        if (entry === null || typeof entry !== "object" || Array.isArray(entry)) continue;
        for (const [wire, name] of fields) {
          const value = (entry as Record<string, unknown>)[wire];
          if (typeof value === "number" || typeof value === "string") actual[name] = value;
        }
      }
    }
  }
  const expected = cursor === null ? { baselineRequired: true } : {
    generation: cursor.generation,
    seqFrom: cursor.seq,
    ...(actual.kind === "CivilUpdate" ? { tickFrom: cursor.tick } : actual.kind === "TickBatch" ? { tickFrom: cursor.tick + 1 } : {}),
  };
  return { actual, expected, cursor };
}

type SnapshotProtocolAction = ReturnType<typeof applyProtocolRuntimeDelta> | ReturnType<typeof installProtocolSnapshotBaseline> | ReturnType<typeof installProtocolWorkingOrdersBaseline>;

export type ProtocolCoordinatorStatus =
  | { readonly kind: "idle" }
  | { readonly kind: "ready"; readonly state: ProtocolState }
  | { readonly kind: "failure"; readonly failure: HostFailure; readonly state: ProtocolState | null };

export interface ProtocolCoordinatorCallbacks {
  readonly onBaseline: (state: ProtocolState, update: Extract<HostUpdate, { type: "baseline" }>) => void;
  readonly onApplied: (reduction: Extract<ProtocolReduction, { kind: "applied" }>, metadata: { readonly civilDate: string | null; readonly revision: string | null }) => void;
  readonly onFailure: (failure: HostFailure) => void;
}

export class ProtocolCoordinator {
  private current: ProtocolState | null = null;
  private currentStatus: ProtocolCoordinatorStatus = { kind: "idle" };
  private readonly callbacks: ProtocolCoordinatorCallbacks;
  private readonly snapshotDispatch: (action: SnapshotProtocolAction) => unknown;

  constructor(callbacks: ProtocolCoordinatorCallbacks, snapshotDispatch: (action: SnapshotProtocolAction) => unknown = store.dispatch) {
    this.callbacks = callbacks;
    this.snapshotDispatch = snapshotDispatch;
  }

  status(): ProtocolCoordinatorStatus {
    return this.currentStatus;
  }

  installPlayerWorkingOrdersBaseline(cursor: ProtocolCursor, value: unknown): void {
    const state = this.current;
    if (state === null || state.cursor.generation !== cursor.generation
      || state.cursor.tick !== cursor.tick || state.cursor.seq !== cursor.seq
      || state.snapshot.tick !== cursor.tick || state.snapshot.seq !== cursor.seq
      || state.playerOrdersReady) {
      throw new ProtocolError("PROTOCOL_CURSOR", "protocol-coordinator.working_orders_baseline", "委托基线必须属于当前 generation、tick 和 seq，且不得覆盖已同步的委托");
    }
    const orders = parseBaselineWorkingOrders(value, state.snapshot.markets);
    this.snapshotDispatch(installProtocolWorkingOrdersBaseline({ ...cursor, orders }));
    const hydrated = { ...state, playerWorkingOrders: orders, playerOrdersReady: true };
    this.current = hydrated;
    this.currentStatus = { kind: "ready", state: hydrated };
  }

  accept(update: HostUpdate): boolean {
    switch (update.type) {
      case "baseline":
        this.installBaseline(update);
        return true;
      case "protocol":
        if (this.currentStatus.kind === "failure") return false;
        this.applyProtocol(update);
        return this.status().kind !== "failure";
      default:
        return assertNever(update);
    }
  }

  fail(failure: HostFailure): void {
    this.currentStatus = { kind: "failure", failure, state: this.current };
    this.callbacks.onFailure(failure);
  }

  private installBaseline(update: Extract<HostUpdate, { type: "baseline" }>): void {
    const state = createProtocolState(update.snapshot, update.generation);
    const hydrated: ProtocolState = {
      ...state,
      securities: update.securities,
      publicPublicationIds: update.publicPublicationIds,
    };
    this.snapshotDispatch(installProtocolSnapshotBaseline({ snapshot: hydrated.snapshot, generation: hydrated.cursor.generation }));
    this.current = hydrated;
    this.currentStatus = { kind: "ready", state: hydrated };
    this.callbacks.onBaseline(hydrated, update);
  }

  private applyProtocol(update: Extract<HostUpdate, { type: "protocol" }>): void {
    if (this.current === null) {
      this.fail({
        code: "PROTOCOL_BASELINE_REQUIRED",
        where: "protocol-coordinator",
        message: "收到协议更新前必须先安装权威基线",
        context: knownProtocolContext(update, null),
      });
      return;
    }
    const attemptCursor = this.current.cursor;
    try {
      const reduction = reduceEngineUpdate(this.current, update.generation, update.update);
      if (reduction.kind === "applied") this.publishSnapshot(reduction);
      this.current = reduction.state;
      this.currentStatus = { kind: "ready", state: reduction.state };
      if (reduction.kind === "applied") {
        this.callbacks.onApplied(reduction, { civilDate: update.civilDate, revision: update.revision });
      }
    } catch (error) {
      if (error instanceof ProtocolError) {
        this.fail({ code: error.code, where: error.where, message: error.message, context: knownProtocolContext(update, attemptCursor) });
        return;
      }
      this.fail({
        code: "PROTOCOL_UNEXPECTED",
        where: "protocol-coordinator",
        message: error instanceof Error ? error.message : String(error),
        context: knownProtocolContext(update, attemptCursor),
      });
    }
  }

  private publishSnapshot(reduction: Extract<ProtocolReduction, { kind: "applied" }>): void {
    const generation = reduction.state.cursor.generation;
    if (reduction.update.kind === "civil-update" || reduction.update.runtimeSnapshot !== null) {
      this.snapshotDispatch(installProtocolSnapshotBaseline({ snapshot: reduction.state.snapshot, generation }));
      return;
    }
    const delta = reduction.update.runtimeDelta;
    if (delta === null) return;
    const finalFrame = reduction.update.frames.at(-1);
    if (finalFrame === undefined) throw new ProtocolError("PROTOCOL_MALFORMED", "protocol-coordinator.runtime_delta", "TickBatch 不得为空");
    this.snapshotDispatch(applyProtocolRuntimeDelta({ generation, delta, markets: finalFrame.markets, activeDailyCandles: finalFrame.activeDailyCandles }));
  }
}

function assertNever(value: never): never {
  throw new Error(`未处理宿主更新：${String(value)}`);
}
