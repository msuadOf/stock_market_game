import type { MobileKlineProjection } from "../mobile/market-model.ts";

export interface CoordinateTick { readonly value: number; readonly y: number; }
export interface KlineTimeTick { readonly label: string; readonly x: number; }

/** 坐标和图形调用同一投影；全零指标只显示真实零点。 */
export function coordinateTicks(min: number, max: number, project: (value: number) => number, count = 5): CoordinateTick[] {
  if (!Number.isFinite(min) || !Number.isFinite(max) || min > max || !Number.isSafeInteger(count) || count < 2) throw new RangeError(`坐标范围或数量无效：min=${min}, max=${max}, count=${count}`);
  if (min === max) return [{ value: min, y: project(min) }];
  return Array.from({ length: count }, (_, index) => {
    const value = max - (max - min) * index / (count - 1);
    return { value, y: project(value) };
  });
}

/** 股是整数，奇数上限的中间刻度取整后重新投影，不能伪造半股。 */
export function klineVolumeTicks(max: number): CoordinateTick[] {
  if (!Number.isSafeInteger(max) || max < 0) throw new RangeError(`成交量坐标上限必须是非负安全整数股：${max}`);
  return [...new Set([max, Math.round(max / 2), 0])].map(value => ({ value, y: max === 0 ? 75 : 75 - value / max * 66 }));
}

/** 小幅值坐标增加精度；接近零的舍入结果不显示负零。 */
export function formatCoordinateValue(value: number, step: number): string {
  if (step > 0 && step < 1e-8) {
    if (value === 0) return "0";
    const digits = Math.min(16, Math.max(2, Math.ceil(Math.log10(Math.abs(value)) - Math.log10(step)) + 1));
    return value.toExponential(digits);
  }
  const digits = Math.min(8, Math.max(2, step > 0 ? Math.ceil(-Math.log10(step)) : 2));
  const text = value.toFixed(digits);
  return Number(text) === 0 ? (0).toFixed(digits) : text;
}

export function klineTradingDayLabel(time: number, compact = true): string {
  const day = Math.floor(time / 86400);
  return day < 0 ? compact ? `前${Math.abs(day)}日` : `开局前${Math.abs(day)}个交易日` : compact ? `第${day + 1}日` : `第${day + 1}个交易日`;
}

/** 标签密度取决于窗口容量，短历史保持原槽宽，不拉伸到整个横轴。 */
export function klineTimeTicks(projection: MobileKlineProjection): KlineTimeTick[] {
  const count = projection.visibleCandles.length;
  if (count === 0) return [];
  const stride = Math.ceil(projection.visibleWindow.capacity / 4);
  const indices: number[] = [];
  for (let index = 0; index < count; index += stride) indices.push(index);
  const last = count - 1;
  if (last - indices[indices.length - 1] >= stride * .65) indices.push(last);
  else if (indices.length > 1) indices[indices.length - 1] = last;
  return indices.map(index => ({ label: klineTradingDayLabel(projection.visibleCandles[index].time), x: projection.slotFor(index).center }));
}
