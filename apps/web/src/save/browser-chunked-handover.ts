import { createChunkedHandoverSources } from "./chunked-handover-source.ts";
import { IndexedDbSaveStorage } from "./indexed-db-save-repository.ts";

const documents = new WeakMap<object, ReturnType<typeof createChunkedHandoverSources>>();

/** 同源各标签用同一锁串行发布与用户删除，避免冻结已经删除的目标。 */
export async function withBrowserPinnedSourceLock<T>(sourceId: string, signal: AbortSignal, action: () => Promise<T>): Promise<T> {
  if (!/^[a-f0-9]{32}$/.test(sourceId)) throw new Error("固定源 sourceId 无效");
  if (globalThis.navigator?.locks === undefined) throw new Error("浏览器不支持重载源跨标签锁，无法安全发布或删除目标");
  return globalThis.navigator.locks.request(`stock-market-game:handover:${sourceId}`, { mode: "exclusive", signal }, action);
}

/** 同一 document 的 owner 和恢复工厂共享唯一的固定源签发者。 */
export function getBrowserChunkedHandoverSources(documentKey: object): ReturnType<typeof createChunkedHandoverSources> {
  const existing = documents.get(documentKey);
  if (existing !== undefined) return existing;
  let storage: IndexedDbSaveStorage | undefined;
  const getStorage = () => storage ??= new IndexedDbSaveStorage(globalThis.indexedDB);
  const sources = createChunkedHandoverSources({
    getItem: key => getStorage().getItem(key),
    hasItem: key => getStorage().hasItem(key),
    addItem: (key, value, current, seed) => getStorage().addItem(key, value, current, seed),
    removeItem: key => getStorage().removeItem(key),
  }, undefined, withBrowserPinnedSourceLock);
  documents.set(documentKey, sources);
  return sources;
}
