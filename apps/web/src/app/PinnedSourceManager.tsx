import { useState } from "react";
import { IndexedDbSaveStorage } from "../save/indexed-db-save-repository.ts";
import { withBrowserPinnedSourceLock } from "../save/browser-chunked-handover.ts";

type Row = { sourceId: string; logicalBytes: number | null; seed: string | null };
type Storage = Pick<IndexedDbSaveStorage, "listPinnedSources" | "deletePinnedSource">;

/** 用户明确撤销所选重载目标；不会触碰快速槽。 */
export function PinnedSourceManager({ createStorage = () => new IndexedDbSaveStorage(globalThis.indexedDB) }: { createStorage?: () => Storage }) {
  const [rows, setRows] = useState<Row[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  async function list() {
    setBusy(true); setError(null);
    try { setRows(await createStorage().listPinnedSources()); setSelected(null); }
    catch (failure) { setError(`读取重载源失败：${failure instanceof Error ? failure.message : String(failure)}`); }
    finally { setBusy(false); }
  }
  async function remove(sourceId: string) {
    setBusy(true); setError(null);
    try {
      await withBrowserPinnedSourceLock(sourceId, new AbortController().signal, () => createStorage().deletePinnedSource(sourceId));
      setRows(previous => previous?.filter(row => row.sourceId !== sourceId) ?? null); setSelected(null);
    } catch (failure) { setError(`删除重载源失败：${failure instanceof Error ? failure.message : String(failure)}`); }
    finally { setBusy(false); }
  }
  return <details><summary>管理重载交接源</summary>
    <p>这里的副本用于页面重载后继续原先选择。删除后，仍等待该目标的其他标签页将无法继续该交接；浏览器快速槽不会被删除。</p>
    <button type="button" disabled={busy} onClick={() => void list()}>查看重载源</button>
    {error !== null && <p role="alert">{error}</p>}
    {rows !== null && (rows.length === 0 ? <p>没有重载源。</p> : <ul>{rows.map(row => <li key={row.sourceId}>
      <span>{row.sourceId} · seed {row.seed ?? "未知"} · {row.logicalBytes === null ? "元数据不完整，大小未知" : `逻辑大小 ${row.logicalBytes} bytes`} </span>
      {selected === row.sourceId ? <><button type="button" disabled={busy} onClick={() => void remove(row.sourceId)}>确认删除此重载源</button><button type="button" disabled={busy} onClick={() => setSelected(null)}>取消</button></>
        : <button type="button" disabled={busy} onClick={() => setSelected(row.sourceId)}>选择删除</button>}
    </li>)}</ul>)}
  </details>;
}
