import type { EngineUpdate } from "../../types/generated/EngineUpdate.ts";
import type { TickFrame } from "../../types/generated/TickFrame.ts";
import { factIdentity } from "./canonical.ts";
import { parseEngineUpdate } from "./parse.ts";
import { ProtocolError, type NormalizedEngineUpdate, type NormalizedTickFrame } from "./types.ts";
import { validateEngineUpdate } from "./validate.ts";

function normalizeFrame(frame: TickFrame): NormalizedTickFrame {
  return {
    tick: frame.tick,
    events: [...frame.events].sort((left, right) => eventSeq(left) - eventSeq(right)),
    facts: [...frame.facts].sort((left, right) => factIdentity(left).localeCompare(factIdentity(right))),
    continuousPoints: frame.timeseries_payload.continuous_points,
    auctionPoints: frame.timeseries_payload.auction_points,
    closedDailyCandles: frame.timeseries_payload.closed_daily_candles,
    activeDailyCandles: frame.timeseries_payload.active_daily_candles,
    markets: frame.timeseries_payload.markets,
    seqFrom: frame.seq_from,
    seqTo: frame.seq_to,
  };
}

function eventSeq(event: TickFrame["events"][number]): number {
  if ("Trade" in event) return event.Trade.seq;
  if ("AuctionTick" in event) return event.AuctionTick.seq;
  if ("AuctionCompleted" in event) return event.AuctionCompleted.seq;
  if ("PriceTick" in event) return event.PriceTick.seq;
  if ("DayBoundary" in event) return event.DayBoundary.seq;
  if ("CivilDateAdvanced" in event) return event.CivilDateAdvanced.seq;
  if ("CompanyDisclosurePublished" in event) return event.CompanyDisclosurePublished.seq;
  if ("IntentRejected" in event) return event.IntentRejected.seq;
  if ("SettlementError" in event) return event.SettlementError.seq;
  if ("ResourceLimit" in event) return event.ResourceLimit.seq;
  if ("OrderCanceled" in event) return event.OrderCanceled.seq;
  if ("OrderAccepted" in event) return event.OrderAccepted.seq;
  throw new ProtocolError("PROTOCOL_MALFORMED", "protocol.normalize", `未处理事件变体：${String(event)}`);
}

export function normalizeEngineUpdate(update: EngineUpdate): NormalizedEngineUpdate {
  if ("TickBatch" in update) {
    return {
      kind: "tick-batch",
      frames: update.TickBatch.frames.map(normalizeFrame),
      runtimeSnapshot: update.TickBatch.runtime_snapshot,
    };
  }
  return {
    kind: "civil-update",
    update: update.CivilUpdate,
    refresh: update.CivilUpdate.refresh,
    intraday: update.CivilUpdate.refresh.intraday.map(normalizeFrame),
  };
}

export function parseNormalizedEngineUpdate(value: unknown): NormalizedEngineUpdate {
  const update = parseEngineUpdate(value);
  validateEngineUpdate(update);
  return normalizeEngineUpdate(update);
}
