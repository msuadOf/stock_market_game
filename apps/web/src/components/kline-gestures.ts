import type { KlineViewportAction } from "../mobile/market-model.ts";

/** 小幅触控板事件累计后再切档，双指距离每变化约 18% 缩放一档。 */
export class KlineGestures {
  private wheelSum = 0;
  private wheelShift = false;
  private distance = 0;
  wheel(delta: number, shift: boolean): KlineViewportAction | null {
    if (shift !== this.wheelShift) this.wheelSum = 0;
    this.wheelShift = shift;
    this.wheelSum += delta;
    if (Math.abs(this.wheelSum) < 40) return null;
    const forward = this.wheelSum > 0;
    this.wheelSum = 0;
    return shift ? (forward ? "pan-right" : "pan-left") : (forward ? "zoom-out" : "zoom-in");
  }
  startPinch(distance: number) { this.distance = distance; }
  pinch(distance: number): KlineViewportAction | null {
    if (this.distance <= 0 || distance <= 0) return null;
    const ratio = distance / this.distance;
    if (ratio < 1.18 && ratio > 1 / 1.18) return null;
    this.distance = distance;
    return ratio > 1 ? "zoom-in" : "zoom-out";
  }
}

export function klineTapIndex(x: number, width: number, capacity: number, count: number): number | null {
  if (width <= 0 || x < 0 || x >= width) return null;
  const index = Math.floor(x / width * capacity);
  return index < count ? index : null;
}
