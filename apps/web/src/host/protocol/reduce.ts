import type { EngineUpdate } from "../../types/generated/EngineUpdate.ts";
import { effectsFromFacts, civilBarrierEffects } from "./effects.ts";
import { normalizeEngineUpdate } from "./normalize.ts";
import { parseEngineUpdate } from "./parse.ts";
import { applyRuntimeDelta } from "./runtime-delta.ts";
import { accountId as canonicalU64 } from "./guards.ts";
import type { AccountId } from "../../types/generated/AccountId.ts";
import {
  type NormalizedEngineUpdate,
  type ProtocolEffect,
  type ProtocolReduction,
  type ProtocolState,
  ProtocolError,
} from "./types.ts";
import { canonicalEngineUpdate, validateEngineUpdate } from "./validate.ts";

function updateRange(update: EngineUpdate): { readonly firstTick: number; readonly tick: number; readonly from: number; readonly to: number; readonly civil: boolean } {
  if ("TickBatch" in update) {
    const first = update.TickBatch.frames[0];
    const last = update.TickBatch.frames.at(-1);
    if (first === undefined || last === undefined) {
      throw new ProtocolError("PROTOCOL_MALFORMED", "protocol.reduce", "TickBatch 不得为空");
    }
    return { firstTick: first.tick, tick: last.tick, from: first.seq_from, to: last.seq_to, civil: false };
  }
  return { firstTick: update.CivilUpdate.tick, tick: update.CivilUpdate.tick, from: update.CivilUpdate.seq_from, to: update.CivilUpdate.seq_to, civil: true };
}

function scopedBaselineAccount(state: ProtocolState): AccountId | null {
  const accounts = Object.keys(state.snapshot.accounts);
  if (accounts.length !== 1) return null;
  return canonicalU64(accounts[0], "protocol.reduce.scopedBaselineAccount");
}

function updateEffects(update: NormalizedEngineUpdate, selfAccountId: AccountId | null): readonly ProtocolEffect[] {
  if (update.kind === "tick-batch") {
    return update.frames.flatMap((frame) => effectsFromFacts(frame.facts, frame.continuousPoints, frame.tick, selfAccountId));
  }
  return [
    ...effectsFromFacts(update.update.facts, {}, update.update.tick, selfAccountId),
    ...civilBarrierEffects(update.update.kinds),
  ];
}

function sameGeneration(state: ProtocolState, generation: string): void {
  if (state.cursor.generation !== generation) {
    throw new ProtocolError("PROTOCOL_CURSOR", "protocol.reduce.generation", "更新代际与当前状态不匹配");
  }
}

function exactRetry(state: ProtocolState, range: ReturnType<typeof updateRange>, canonical: string): boolean {
  const accepted = state.accepted.get(`${range.tick}\u0000${range.to}`);
  if (accepted === undefined) return false;
  if (accepted === canonical) return true;
  throw new ProtocolError("PROTOCOL_REPLAY", "protocol.reduce.replay", "相同结束游标包含不同规范更新");
}

function expectedCursor(state: ProtocolState, range: ReturnType<typeof updateRange>): void {
  const expectedTick = range.civil ? state.cursor.tick : state.cursor.tick + 1;
  if (range.firstTick !== expectedTick || range.from !== state.cursor.seq) {
    throw new ProtocolError("PROTOCOL_CURSOR", "protocol.reduce.cursor", "更新起始 tick 或 seq 游标不连续");
  }
}

function appliedState(state: ProtocolState, generation: string, update: EngineUpdate, canonical: string): ProtocolState {
  const range = updateRange(update);
  const accepted = new Map(state.accepted);
  accepted.set(`${range.tick}\u0000${range.to}`, canonical);
  if ("TickBatch" in update) {
    const batch = update.TickBatch;
    const normalized = normalizeEngineUpdate(update);
    if (normalized.kind !== "tick-batch") {
      throw new ProtocolError("PROTOCOL_MALFORMED", "protocol.reduce", "TickBatch 归一化类型不匹配");
    }
    const frames = normalized.frames;
    const runtimeSnapshot = batch.runtime_snapshot;
    const finalFrame = frames.at(-1);
    if (finalFrame === undefined) throw new ProtocolError("PROTOCOL_MALFORMED", "protocol.reduce", "TickBatch 不得为空");
    const deltaState = batch.runtime_delta !== undefined
      ? applyRuntimeDelta(state, batch.runtime_delta, finalFrame.markets, finalFrame.activeDailyCandles)
      : runtimeSnapshot === null ? null : { snapshot: runtimeSnapshot, playerWorkingOrders: {}, playerOrdersReady: false };
    return {
      ...state,
      cursor: { generation, tick: range.tick, seq: range.to },
      snapshot: runtimeSnapshot ?? state.snapshot,
      ...(deltaState === null ? {} : deltaState),
      intraday: [...state.intraday, ...frames],
      accepted,
    };
  }
  const civil = update.CivilUpdate;
  const normalized = normalizeEngineUpdate(update);
  if (normalized.kind !== "civil-update") {
    throw new ProtocolError("PROTOCOL_MALFORMED", "protocol.reduce", "CivilUpdate 归一化类型不匹配");
  }
  return {
    ...state,
    cursor: { generation, tick: range.tick, seq: range.to },
    snapshot: civil.refresh.snapshot,
    securities: civil.refresh.securities,
    publicPublicationIds: civil.refresh.public_publication_ids,
    intraday: normalized.intraday,
    accepted,
    playerWorkingOrders: {},
    playerOrdersReady: false,
  };
}

export function reduceEngineUpdate(
  state: ProtocolState,
  generation: string,
  unknownUpdate: unknown,
): ProtocolReduction {
  sameGeneration(state, generation);
  const update = parseEngineUpdate(unknownUpdate);
  validateEngineUpdate(update);
  const range = updateRange(update);
  const canonical = canonicalEngineUpdate(update);
  const normalizedUpdate = normalizeEngineUpdate(update);
  if (exactRetry(state, range, canonical)) {
    return { kind: "exact-retry", state, update: normalizedUpdate, effects: [] };
  }
  expectedCursor(state, range);
  return {
    kind: "applied",
    state: appliedState(state, generation, update, canonical),
    update: normalizedUpdate,
    effects: updateEffects(normalizedUpdate, scopedBaselineAccount(state)),
  };
}
