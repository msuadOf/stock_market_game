export function assertValidSpeedMultiplier(speed: number): void {
  if (speed === Infinity) return;
  if (!Number.isFinite(speed) || speed <= 0) {
    throw new Error(`非法速度倍率：${speed}（必须为正数或 Infinity）`);
  }
}
