import * as React from "react";

/** 定向测试只模拟 React 的 state/ref/memo，不模拟 DOM 或浏览器提交周期。 */
export function memoryHook<Options, Result>(hook: (options: Options) => Result, options: Options) {
  const slots: { value: unknown; dependencies?: readonly unknown[] }[] = [];
  let cursor = 0;
  const internals = (React as unknown as { __SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED: { ReactCurrentDispatcher: { current: unknown } } }).__SECRET_INTERNALS_DO_NOT_USE_OR_YOU_WILL_BE_FIRED;
  function memo<Value>(create: () => Value, dependencies: readonly unknown[]) {
    const index = cursor++;
    const slot = slots[index];
    if (slot === undefined || slot.dependencies?.length !== dependencies.length || dependencies.some((value, position) => !Object.is(value, slot.dependencies![position]))) {
      slots[index] = { value: create(), dependencies };
    }
    return slots[index].value as Value;
  }
  const dispatcher = {
    useState<Value>(initial: Value | (() => Value)) {
      const index = cursor++;
      if (slots[index] === undefined) slots[index] = { value: typeof initial === "function" ? (initial as () => Value)() : initial };
      return [slots[index].value, (next: Value | ((previous: Value) => Value)) => {
        slots[index].value = typeof next === "function" ? (next as (previous: Value) => Value)(slots[index].value as Value) : next;
      }];
    },
    useRef<Value>(initial: Value) { return memo(() => ({ current: initial }), []); },
    useMemo: memo,
    useCallback<Value>(callback: Value, dependencies: readonly unknown[]) { return memo(() => callback, dependencies); },
  };
  return {
    render(nextOptions = options): Result {
      options = nextOptions;
      const previous = internals.ReactCurrentDispatcher.current;
      internals.ReactCurrentDispatcher.current = dispatcher;
      cursor = 0;
      try { return hook(options); } finally { internals.ReactCurrentDispatcher.current = previous; }
    },
  };
}
