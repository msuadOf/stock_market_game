interface ClockSnapshot {
  tick: number;
}

/** 从事件批次同步权威 tick；乱序旧事件不得让游戏时钟倒退。 */
export function syncSnapshotTick(snapshot: ClockSnapshot | null, events: readonly unknown[]): void {
  if (!snapshot) return;
  let latestTick = snapshot.tick;
  for (const event of events) {
    if (typeof event !== "object" || event === null || !("PriceTick" in event)) continue;
    const priceTick = (event as { PriceTick?: unknown }).PriceTick;
    if (typeof priceTick !== "object" || priceTick === null || !("tick" in priceTick)) {
      throw new TypeError("PriceTick.tick 缺失，无法同步游戏时钟");
    }
    const tick = (priceTick as { tick?: unknown }).tick;
    if (!Number.isSafeInteger(tick) || (tick as number) < 0) {
      throw new RangeError(`PriceTick.tick 必须是非负安全整数，收到 ${String(tick)}`);
    }
    latestTick = Math.max(latestTick, tick as number);
  }
  snapshot.tick = latestTick;
}
