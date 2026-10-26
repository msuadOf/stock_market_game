import { useEffect, useRef, useState } from "react";

export function useCompanyQuery<T>(query: (() => Promise<T>) | undefined, refreshKey: string): readonly [T | null, string | null, boolean, () => void] {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const sequence = useRef(0);
  const queryRef = useRef(query);
  queryRef.current = query;
  const refresh = useRef(() => {});
  refresh.current = () => {
    const current = queryRef.current;
    if (current === undefined) return;
    const ticket = ++sequence.current;
    setData(null);
    setError(null);
    setLoading(true);
    current().then(
      (result) => {
        if (sequence.current === ticket) { setData(result); setLoading(false); }
      },
      (failure) => {
        if (sequence.current === ticket) {
          setError(failure instanceof Error ? failure.message : String(failure));
          setLoading(false);
        }
      },
    );
  };
  // 触发键先提成具名变量（oxlint exhaustive-deps 要求依赖数组可静态检查）；
  // 语义等价：支持位或刷新键变化才重新查询，闭包身份不参与。
  const trigger = JSON.stringify([query !== undefined, refreshKey]);
  useEffect(() => {
    refresh.current();
  }, [trigger]);
  return [data, error, loading, () => refresh.current()] as const;
}

