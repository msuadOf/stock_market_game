import type { KlinePoint } from "../components/PriceChart.tsx";

export class StockHistoryRequestGate {
  private requestId = 0;

  begin(): number {
    this.requestId += 1;
    return this.requestId;
  }

  isCurrent(requestId: number): boolean {
    return requestId === this.requestId;
  }
}

export function mergeStockHistoryCandles(queried: readonly KlinePoint[], current: readonly KlinePoint[]): readonly KlinePoint[] {
  const candlesByTime = new Map(queried.map((candle) => [candle.time, candle]));
  for (const candle of current) candlesByTime.set(candle.time, candle);
  return [...candlesByTime.values()].sort((left, right) => left.time - right.time);
}
