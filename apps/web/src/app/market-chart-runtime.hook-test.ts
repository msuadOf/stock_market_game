/* oxlint-disable react-hooks/rules-of-hooks -- 测试用 React Dispatcher 驱动 Hook 重渲染。 */
import React, { type Dispatch, type MutableRefObject, type SetStateAction } from "react";
import type { EngineHost } from "../host/engine-host.ts";
import { useMarketChartRuntime } from "./useMarketChartRuntime.ts";

export function createMarketChartRuntimeHarness({ hostRef, setNotice }: {
  readonly hostRef: MutableRefObject<EngineHost | null>;
  readonly setNotice: Dispatch<SetStateAction<string | null>>;
}) {
  const state: unknown[] = [];
  let cursor = 0;
  const dispatcher = {
    useState<T>(initial: T | (() => T)) {
      const slot = cursor++;
      if (!(slot in state)) state[slot] = typeof initial === "function" ? (initial as () => T)() : initial;
      const update: Dispatch<SetStateAction<T>> = (next) => {
        state[slot] = typeof next === "function" ? (next as (previous: T) => T)(state[slot] as T) : next;
      };
      return [state[slot] as T, update] as const;
    },
    useRef<T>(initial: T) {
      const slot = cursor++;
      if (!(slot in state)) state[slot] = { current: initial };
      return state[slot] as MutableRefObject<T>;
    },
    useCallback(callback: unknown) {
      cursor += 1;
      return callback;
    },
  };
  const internals = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  return {
    render() {
      const previous = internals.ReactCurrentDispatcher.current;
      internals.ReactCurrentDispatcher.current = dispatcher;
      cursor = 0;
      try {
        return useMarketChartRuntime({ autoOrderManagerRef: { current: null }, hostRef, setNotice });
      } finally {
        internals.ReactCurrentDispatcher.current = previous;
      }
    },
  };
}
