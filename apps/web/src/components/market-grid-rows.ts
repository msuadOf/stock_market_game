import type { Cents, MarketSnap } from "../types/engine.ts";
import { STOCK_NAMES } from "../config/defaults.ts";
import { priceChangePercent } from "../mobile/market-model.ts";

export interface MarketGridRow {
  code: string;
  name: string;
  lastPrice: number;
  changeAbs: number;
  changePct: number;
  _rawLastPrice: Cents;
  _rawLastClose: Cents;
  _source: MarketSnap;
}

export interface MarketRowTransaction {
  add: MarketGridRow[];
  update: MarketGridRow[];
  remove: MarketGridRow[];
}

export function buildMarketRows(
  markets: Readonly<Record<string, MarketSnap>>,
  codes: readonly string[],
  previous: readonly MarketGridRow[] = [],
): MarketGridRow[] {
  const previousByCode = new Map(previous.map((row) => [row.code, row]));
  return codes.flatMap((code) => {
    const market = markets[code];
    if (!market) return [];
    const oldRow = previousByCode.get(code);
    if (oldRow?._source === market) return [oldRow];
    const difference = market.last_price - market.last_close;
    return [{
      code,
      name: STOCK_NAMES[code] ?? code,
      lastPrice: market.last_price / 100,
      changeAbs: difference / 100,
      changePct: priceChangePercent(market.last_price, market.last_close),
      _rawLastPrice: market.last_price,
      _rawLastClose: market.last_close,
      _source: market,
    }];
  });
}

export function diffMarketRows(
  previous: readonly MarketGridRow[],
  next: readonly MarketGridRow[],
): MarketRowTransaction {
  const previousByCode = new Map(previous.map((row) => [row.code, row]));
  const nextByCode = new Map(next.map((row) => [row.code, row]));
  return {
    add: next.filter((row) => !previousByCode.has(row.code)),
    update: next.filter((row) => {
      const oldRow = previousByCode.get(row.code);
      return oldRow !== undefined && oldRow !== row;
    }),
    remove: previous.filter((row) => !nextByCode.has(row.code)),
  };
}
