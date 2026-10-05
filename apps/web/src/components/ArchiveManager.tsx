import { useEffect, useRef, useState } from "react";
import type { ArchiveMetadata, ArchiveStore } from "../save/archive-store.ts";

interface Props {
  repository: ArchiveStore;
  onLoad(slotId: string): Promise<void>;
  onClose(): void;
}

export function ArchiveManager({ repository, onLoad, onClose }: Props) {
  const [entries, setEntries] = useState<ArchiveMetadata[]>([]);
  const [name, setName] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const epoch = useRef(0);

  useEffect(() => {
    const generation = ++epoch.current;
    setBusy(true);
    void repository.list().then((items) => {
      if (generation === epoch.current) setEntries(items);
    }, (failure: unknown) => {
      if (generation === epoch.current) setError(`读取存档列表失败：${failure instanceof Error ? failure.message : String(failure)}；请检查存储权限并反馈错误。`);
    }).finally(() => { if (generation === epoch.current) setBusy(false); });
    return () => { epoch.current += 1; };
  }, [repository]);

  async function manage(operation: () => Promise<unknown>) {
    if (busy) return;
    const generation = epoch.current;
    setBusy(true);
    setError(null);
    try {
      await operation();
      const items = await repository.list();
      if (generation === epoch.current) setEntries(items);
    } catch (failure) {
      if (generation === epoch.current) setError(`存档操作失败：${failure instanceof Error ? failure.message : String(failure)}；原有效档不会因失败写入而被替换，请反馈错误。`);
    } finally {
      if (generation === epoch.current) setBusy(false);
    }
  }

  return <section role="dialog" aria-modal="false" aria-label="日终存档管理" className="archive-manager">
    <h2>日终存档管理</h2>
    <p>列表和槽位管理不会重新加载市场；只有明确读档才替换当前局。日内状态不写入存档。</p>
    <label>新名称<input aria-label="存档新名称" value={name} onChange={(event) => setName(event.currentTarget.value)} disabled={busy} /></label>
    {error !== null && <p role="alert">{error}</p>}
    {entries.length === 0 && <p>{busy ? "读取存档中…" : "还没有成功日终存档"}</p>}
    <ul>{entries.map((entry) => <li key={entry.slot_id}>
      <span>{entry.name} · {entry.civil_date} · tick {entry.tick}</span>
      <button disabled={busy} onClick={() => void manage(() => onLoad(entry.slot_id))}>读档</button>
      <button disabled={busy || name.trim() === ""} onClick={() => void manage(() => repository.rename(entry.slot_id, name))}>重命名</button>
      <button disabled={busy || name.trim() === ""} onClick={() => void manage(() => repository.copy(entry.slot_id, name))}>复制</button>
      <button disabled={busy} onClick={() => { if (window.confirm(`确定删除日终存档“${entry.name}”？此操作不可撤销。`)) void manage(() => repository.delete(entry.slot_id)); }}>删除</button>
    </li>)}</ul>
    <button onClick={onClose}>关闭</button>
  </section>;
}
