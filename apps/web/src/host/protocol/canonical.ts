import type { Event } from "../../types/generated/Event.ts";
import type { EventFact } from "../../types/generated/EventFact.ts";
import { isRecord } from "./guards.ts";
import { ProtocolError } from "./types.ts";

function unsupported(value: unknown): never {
  throw new ProtocolError(
    "PROTOCOL_MALFORMED",
    "protocol.canonical",
    `协议包含不支持的值：${typeof value}`,
  );
}

function stableValue(value: unknown): string {
  switch (typeof value) {
    case "string":
      return JSON.stringify(value);
    case "number":
      if (!Number.isFinite(value)) unsupported(value);
      return JSON.stringify(value);
    case "boolean":
      return JSON.stringify(value);
    case "object":
      if (value === null) return "null";
      if (Array.isArray(value)) return `[${value.map(stableValue).join(",")}]`;
      if (isRecord(value)) {
        return `{${Object.keys(value)
          .sort()
          .map((key) => `${JSON.stringify(key)}:${stableValue(value[key])}`)
          .join(",")}}`;
      }
      return unsupported(value);
    default:
      return unsupported(value);
  }
}

export function canonicalJson(value: unknown): string {
  return stableValue(value);
}

export function eventStableKey(event: Event, localEventIndex: number): EventFact["key"] {
  if ("Trade" in event) {
    return { phase_rank: 4, entity: { Stock: event.Trade.code }, source: "Sealed", local_event_index: localEventIndex };
  }
  if ("AuctionTick" in event) {
    return { phase_rank: 4, entity: { Stock: event.AuctionTick.code }, source: "PriceTick", local_event_index: localEventIndex };
  }
  if ("AuctionCompleted" in event) {
    return { phase_rank: 4, entity: { Stock: event.AuctionCompleted.code }, source: "PriceTick", local_event_index: localEventIndex };
  }
  if ("PriceTick" in event) {
    return { phase_rank: 4, entity: { Stock: event.PriceTick.code }, source: "PriceTick", local_event_index: localEventIndex };
  }
  if ("DayBoundary" in event) {
    return { phase_rank: 5, entity: "Session", source: "DayEnd", local_event_index: localEventIndex };
  }
  if ("CivilDateAdvanced" in event || "CompanyDisclosurePublished" in event) {
    return { phase_rank: 6, entity: "Session", source: "Session", local_event_index: localEventIndex };
  }
  if ("IntentRejected" in event) {
    return { phase_rank: 4, entity: { Account: event.IntentRejected.account }, source: "Sealed", local_event_index: localEventIndex };
  }
  if ("SettlementError" in event) {
    return { phase_rank: 4, entity: { Account: event.SettlementError.account }, source: "Sealed", local_event_index: localEventIndex };
  }
  if ("OrderCanceled" in event) {
    return { phase_rank: 4, entity: { Account: event.OrderCanceled.account }, source: "Sealed", local_event_index: localEventIndex };
  }
  if ("OrderAccepted" in event) {
    return { phase_rank: 4, entity: { Account: event.OrderAccepted.account }, source: "Sealed", local_event_index: localEventIndex };
  }
  return assertNever(event);
}

function assertNever(value: never): never {
  throw new ProtocolError("PROTOCOL_MALFORMED", "protocol.canonical", `未处理协议变体：${String(value)}`);
}

export function factIdentity(fact: EventFact): string {
  return canonicalJson(fact.key);
}

export function canonicalFacts(facts: readonly EventFact[]): readonly EventFact[] {
  const identities = new Set<string>();
  for (const fact of facts) {
    const identity = factIdentity(fact);
    if (identities.has(identity)) {
      throw new ProtocolError("PROTOCOL_MALFORMED", "protocol.canonicalFacts", "协议事实稳定身份重复");
    }
    identities.add(identity);
  }
  return [...facts].sort((left, right) => factIdentity(left).localeCompare(factIdentity(right)));
}
