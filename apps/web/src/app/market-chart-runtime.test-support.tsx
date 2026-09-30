import { createElement, useRef } from "react";
import { useMarketChartRuntime } from "./useMarketChartRuntime.ts";
import { ProtocolCoordinator } from "../host/protocol-coordinator.ts";
import type { HostUpdate } from "../host/host-update.ts";
import type { KlinePoint } from "../components/PriceChart.tsx";

export type ChartRuntimeObservation = {
  readonly daily: readonly KlinePoint[];
  readonly active: Readonly<Record<string, KlinePoint>>;
};

export function MarketChartRuntimeProbe({ baseline, updates, observe }: {
  readonly baseline: Extract<HostUpdate, { type: "baseline" }>;
  readonly updates: readonly Extract<HostUpdate, { type: "protocol" }>[];
  readonly observe: (stage: "baseline" | "updated", observation: ChartRuntimeObservation) => void;
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
    coordinator.current.accept(baseline);
  } else if (stage.current === 1) {
    stage.current = 2;
    observe("baseline", { daily: runtime.dailyChartData, active: runtime.activeDailyCandlesRef.current });
    for (const update of updates) coordinator.current.accept(update);
  } else {
    observe("updated", { daily: runtime.dailyChartData, active: runtime.activeDailyCandlesRef.current });
  }
  return createElement("output", null, JSON.stringify(runtime.dailyChartData));
}
