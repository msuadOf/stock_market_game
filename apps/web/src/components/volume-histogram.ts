import type { HistogramData, UTCTimestamp } from "lightweight-charts";
import type { PricePoint } from "./PriceChart.tsx";

export function volumeHistogramData(data: readonly PricePoint[]): HistogramData<UTCTimestamp>[] {
  return data.map((point) => ({
    time: point.time as UTCTimestamp,
    value: (point.volume ?? 0) / 100,
    color: point.buy ? "rgba(216,30,6,0.5)" : "rgba(0,153,68,0.5)",
  }));
}
