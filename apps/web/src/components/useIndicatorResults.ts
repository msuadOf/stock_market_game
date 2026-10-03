import { useEffect, useRef, useState } from "react";
import {
  IndicatorRequestGate,
  IndicatorResultRequest,
  type IndicatorCalculator,
  type IndicatorInput,
  type IndicatorResultRecord,
  type IndicatorResultState,
} from "./indicator-results.ts";

export type { IndicatorResultState } from "./indicator-results.ts";

export function useIndicatorResults(
  calculator: IndicatorCalculator | null,
  input: IndicatorInput,
  enabled: boolean,
): IndicatorResultState {
  const gateRef = useRef<IndicatorRequestGate | null>(null);
  if (gateRef.current === null) gateRef.current = new IndicatorRequestGate();
  const [requestState, setRequestState] = useState<IndicatorResultRecord | null>(null);

  useEffect(() => {
    const gate = gateRef.current;
    if (gate === null) throw new Error("指标请求门尚未初始化");
    if (!enabled || calculator === null) return () => gate.invalidate();

    const request = IndicatorResultRequest.capture(gate, calculator, input);
    setRequestState(request.pendingRecord());
    void Promise.resolve().then(() => calculator(input)).then(
      (response) => {
        const record = request.resolveRecord(response);
        if (record !== null) setRequestState(record);
      },
      (error: unknown) => {
        const record = request.rejectRecord(error);
        if (record !== null) setRequestState(record);
      },
    );
    return () => gate.invalidate();
  }, [calculator, enabled, input]);

  if (!enabled) return { kind: "idle" };
  if (calculator === null) return { kind: "unavailable" };
  if (requestState === null || !requestState.request.matches(calculator, input)) return { kind: "pending" };
  return requestState.state;
}
