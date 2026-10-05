import type { IndicatorDataSource } from "../store/store.ts";

export type IndicatorName = "intradayAverage" | "macd" | "priceKdj" | "candleKdj";

export interface IndicatorCapabilities {
  readonly intradayAverage: boolean;
  readonly macd: boolean;
  readonly priceKdj: boolean;
  readonly candleKdj: boolean;
}

export type IndicatorRoute =
  | { readonly kind: "frontend" }
  | { readonly kind: "rust" }
  | { readonly kind: "unsupported"; readonly requested: IndicatorDataSource; readonly indicator: IndicatorName };

export function resolveIndicatorRoute(
  source: IndicatorDataSource,
  indicator: IndicatorName,
  capabilities: IndicatorCapabilities,
): IndicatorRoute {
  if (source === "frontend") {
    return indicator === "intradayAverage"
      ? { kind: "frontend" }
      : { kind: "unsupported", requested: source, indicator };
  }
  return capabilities[indicator]
    ? { kind: "rust" }
    : { kind: "unsupported", requested: source, indicator };
}
