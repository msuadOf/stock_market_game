interface ClockSnapshot {
  tick: number;
}

/** 从事件批次同步权威 tick；乱序旧事件不得让游戏时钟倒退。 */
export function syncSnapshotTick(snapshot: ClockSnapshot | null, events: readonly unknown[]): void {
  if (!snapshot) return;
  let latestTick = snapshot.tick;
  for (const event of events) {
    if (typeof event !== "object" || event === null) continue;
    const kind = "PriceTick" in event ? "PriceTick" : "AuctionTick" in event ? "AuctionTick" : "AuctionCompleted" in event ? "AuctionCompleted" : null;
    if (!kind) continue;
    const tickEvent = (event as Record<string, unknown>)[kind];
    if (typeof tickEvent !== "object" || tickEvent === null || !("tick" in tickEvent)) {
      throw new TypeError(`${kind}.tick 缺失，无法同步游戏时钟`);
    }
    const tick = (tickEvent as { tick?: unknown }).tick;
    if (!Number.isSafeInteger(tick) || (tick as number) < 0) {
      throw new RangeError(`${kind}.tick 必须是非负安全整数，收到 ${String(tick)}`);
    }
    latestTick = Math.max(latestTick, tick as number);
  }
  snapshot.tick = latestTick;
}
