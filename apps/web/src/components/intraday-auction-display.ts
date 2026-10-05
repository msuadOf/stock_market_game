import { intradayChartX, type AuctionPoint } from "../mobile/market-model.ts";

/** 无指示价时仅在显示层使用昨收参考值；原始行情不补价，更新点不代表竞价已成交。 */
export function auctionDisplayPoints(points: readonly AuctionPoint[], lastClose: number) {
  return points.map((point, index) => {
    const previous = points[index - 1];
    return { ...point, value: point.value === null ? lastClose : point.value,
      updated: point.value !== null && (previous === undefined || previous.value !== point.value || previous.volume !== point.volume),
    };
  });
}

/** 只连接09:30同一坐标的两条已有记录；缺失端点不延长曲线或补价。 */
export function auctionContinuousJoin(auction: readonly Readonly<{ time: number; value: number }>[], continuous: readonly Readonly<{ time: number; value: number }>[]): readonly Readonly<{ x: number; value: number }>[] {
  const last = auction.at(-1);
  const first = continuous[0];
  if (last === undefined || first === undefined || last.value === first.value) return [];
  const auctionX = intradayChartX({ phase: "auction", minute: last.time });
  const continuousX = intradayChartX({ phase: "continuous", minute: first.time });
  if (auctionX !== continuousX) return [];
  return [{ x: auctionX, value: last.value }, { x: continuousX, value: first.value }];
}
