import type { AuctionPoint } from "../mobile/market-model.ts";

/** 无指示价时仅在显示层使用昨收参考值；原始行情不补价，更新点不代表竞价已成交。 */
export function auctionDisplayPoints(points: readonly AuctionPoint[], lastClose: number) {
  return points.map((point, index) => {
    const previous = points[index - 1];
    return { ...point, value: point.value === null ? lastClose : point.value,
      updated: point.value !== null && (previous === undefined || previous.value !== point.value || previous.volume !== point.volume),
    };
  });
}
