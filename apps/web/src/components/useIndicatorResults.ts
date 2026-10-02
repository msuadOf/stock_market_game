import { useEffect, useRef, useState } from "react";
import {
  IndicatorRequestGate,
  parseIndicatorResults,
  type IndicatorCalculator,
  type IndicatorInput,
  type IndicatorResults,
} from "./indicator-results.ts";

export type IndicatorResultState =
  | { kind: "idle" }
  | { kind: "unavailable" }
  | { kind: "pending" }
  | { kind: "ready"; value: IndicatorResults }
  | { kind: "error"; message: string };

function describeIndicatorFailure(error: unknown): string {
  return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}

export function useIndicatorResults(
  calculator: IndicatorCalculator | null,
  input: IndicatorInput,
  enabled: boolean,
): IndicatorResultState {
  const gateRef = useRef<IndicatorRequestGate | null>(null);
  if (gateRef.current === null) gateRef.current = new IndicatorRequestGate();
  const [requestState, setRequestState] = useState<{
    readonly calculator: IndicatorCalculator;
    readonly input: IndicatorInput;
    readonly state: IndicatorResultState;
  } | null>(null);

  useEffect(() => {
    const gate = gateRef.current;
    if (gate === null) throw new Error("指标请求门尚未初始化");
    const generation = gate.capture();
    if (!enabled || calculator === null) return () => gate.invalidate();

    const update = (state: IndicatorResultState) => setRequestState({ calculator, input, state });
    update({ kind: "pending" });
    void Promise.resolve().then(() => calculator(input)).then(
      (response) => {
        if (!gate.isCurrent(generation)) return;
        try {
          const value = parseIndicatorResults(response, input.prices.length, input.candles?.length ?? 0);
          update({ kind: "ready", value });
        } catch (error) {
          update({ kind: "error", message: describeIndicatorFailure(error) });
        }
      },
      (error: unknown) => {
        if (gate.isCurrent(generation)) update({ kind: "error", message: describeIndicatorFailure(error) });
      },
    );
    return () => gate.invalidate();
  }, [calculator, enabled, input]);

  if (!enabled) return { kind: "idle" };
  if (calculator === null) return { kind: "unavailable" };
  if (requestState?.calculator !== calculator || requestState.input !== input) return { kind: "pending" };
  return requestState.state;
}
