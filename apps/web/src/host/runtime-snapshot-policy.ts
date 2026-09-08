import type { EngineEvent, Snapshot } from "../types/engine";

/**
 * 账户权威状态不在事件中重复携带；成交、挂单/撤单造成的资金与股份冻结变化、
 * 集合竞价余单转簿或跨日解锁后，都必须同步轻量快照。
 */
export function requiresRuntimeSnapshot(events: EngineEvent[]): boolean {
  return events.some(
    (event) =>
      "Trade" in event ||
      "OrderAccepted" in event ||
      "OrderCanceled" in event ||
      "AuctionCompleted" in event ||
      "DayBoundary" in event,
  );
}

/**
 * 同一步的事件描述状态变化，运行时快照描述该变化后的权威结果；消费者必须按因果顺序接收。
 */
export function deliverEventsThenSnapshot(
  events: EngineEvent[],
  runtimeSnapshot: Snapshot | undefined,
  onEvents: ((events: EngineEvent[]) => void) | null,
  onSnapshot: ((snapshot: Snapshot) => void) | null,
): void {
  if (events.length > 0 && onEvents) {
    onEvents(events);
  }
  if (runtimeSnapshot !== undefined && onSnapshot) {
    onSnapshot(runtimeSnapshot);
  }
}
