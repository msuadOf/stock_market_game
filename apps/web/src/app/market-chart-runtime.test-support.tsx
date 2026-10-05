import { createElement, useRef } from "react";
import { useMarketChartRuntime } from "./useMarketChartRuntime.ts";
import { ProtocolCoordinator } from "../host/protocol-coordinator.ts";
import type { HostUpdate } from "../host/host-update.ts";
import type { KlinePoint, PricePoint } from "../components/PriceChart.tsx";
import type { AuctionPoint } from "../mobile/market-model.ts";
import type { TradingTiming } from "../components/trading-timeline.ts";

export type ChartRuntimeObservation = {
  readonly daily: readonly KlinePoint[];
  readonly active: Readonly<Record<string, KlinePoint>>;
  readonly prices: readonly PricePoint[];
  readonly auctions: readonly AuctionPoint[];
  readonly history: Readonly<Record<string, readonly PricePoint[]>>;
  readonly code: string;
};

export function MarketChartRuntimeProbe({ baseline, updates, observe, afterUpdates, timing }: {
  readonly timing: TradingTiming;
  readonly baseline: Extract<HostUpdate, { type: "baseline" }>;
  readonly updates: readonly Extract<HostUpdate, { type: "protocol" }>[];
  readonly observe: (stage: "baseline" | "updated", observation: ChartRuntimeObservation) => void;
  readonly afterUpdates?: (runtime: ReturnType<typeof useMarketChartRuntime>) => void;
}) {
  const runtime = useMarketChartRuntime({ autoOrderManagerRef: { current: null }, setNotice: () => {} });
  const latestRuntime = useRef(runtime);
  latestRuntime.current = runtime;
  const coordinator = useRef<ProtocolCoordinator | null>(null);
  const stage = useRef(0);
  if (coordinator.current === null) {
    coordinator.current = new ProtocolCoordinator({
      onBaseline: (state) => latestRuntime.current.installBaseline(state),
      onApplied: (reduction) => latestRuntime.current.acceptReduction(reduction),
      onFailure: (failure) => { throw new Error(`${failure.code}: ${failure.message}`); },
    });
  }
  if (stage.current === 0) {
    stage.current = 1;
    runtime.configureMarketTiming(timing);
    coordinator.current.accept(baseline);
  } else if (stage.current === 1) {
    stage.current = 2;
    observe("baseline", observation(runtime));
    for (const update of updates) coordinator.current.accept(update);
    afterUpdates?.(runtime);
  } else {
    observe("updated", observation(runtime));
  }
  return createElement("output", null, JSON.stringify(runtime.dailyChartData));
}

function observation(runtime: ReturnType<typeof useMarketChartRuntime>): ChartRuntimeObservation {
  return { daily: runtime.dailyChartData, active: runtime.getActiveDailyCandles(), prices: runtime.chartData,
    auctions: runtime.auctionChartData, history: runtime.getPriceHistory(), code: runtime.chartCode };
}
