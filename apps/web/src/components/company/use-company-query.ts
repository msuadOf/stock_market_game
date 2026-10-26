import { useCallback, useEffect, useRef, useState } from "react";

type QueryKey = string | readonly (string | number | null)[];

/** 每次请求绑定 callback 与上下文；变更、移除或卸载均使旧成功和错误失效。 */
export function useCompanyQuery<T>(query: (() => Promise<T>) | undefined, refreshKey: QueryKey): readonly [T | null, string | null, boolean, () => void] {
  const key = JSON.stringify(refreshKey);
  const [state, setState] = useState<{ query: typeof query; key: string; data: T | null; error: string | null; loading: boolean }>({ query, key, data: null, error: null, loading: false });
  const sequence = useRef(0);
  const refresh = useCallback(() => {
    const ticket = ++sequence.current;
    setState({ query, key, data: null, error: null, loading: query !== undefined });
    if (query === undefined) return;
    Promise.resolve().then(query).then(
      (data) => { if (sequence.current === ticket) setState({ query, key, data, error: null, loading: false }); },
      (failure: unknown) => {
        if (sequence.current === ticket) setState({ query, key, data: null, error: failure instanceof Error ? failure.message : String(failure), loading: false });
      },
    );
  }, [query, key]);
  useEffect(() => {
    refresh();
    return () => { sequence.current += 1; };
  }, [refresh]);
  const current = state.query === query && state.key === key;
  return [current ? state.data : null, current ? state.error : null, current && state.loading, refresh];
}
