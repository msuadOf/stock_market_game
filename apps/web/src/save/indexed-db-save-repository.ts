import { CompressedSaveRepository, gzipSaveCodec, type AsyncSaveStorage, type SaveCompressionCodec } from "./save-repository.ts";

import { SAVE_DATABASE, SAVE_OBJECT_STORE, QUICK_SAVE_KEY } from "./save-storage-keys.ts";
export { SAVE_DATABASE, SAVE_OBJECT_STORE, QUICK_SAVE_KEY } from "./save-storage-keys.ts";
const PIN_PREFIX = "wasm-handover-source:";
const PIN_META_PREFIX = "wasm-handover-meta:";

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export class IndexedDbSaveStorage implements AsyncSaveStorage {
  private connection: Promise<IDBDatabase> | null = null;
  private readonly factory: IDBFactory;
  private readonly database: string;
  private readonly maximumPinnedBytes: number;

  constructor(factory: IDBFactory, database = SAVE_DATABASE, maximumPinnedBytes = 2 * 1024 * 1024 * 1024) {
    if (factory === undefined || factory === null) throw new Error("当前浏览器不支持 IndexedDB，无法保存本地快速槽");
    this.factory = factory;
    this.database = database;
    if (!Number.isSafeInteger(maximumPinnedBytes) || maximumPinnedBytes <= 0) throw new Error("固定源容量上限必须为正整数");
    this.maximumPinnedBytes = maximumPinnedBytes;
  }

  private open(): Promise<IDBDatabase> {
    if (this.connection !== null) return this.connection;
    const pending = new Promise<IDBDatabase>((resolve, reject) => {
      const request = this.factory.open(this.database, 1);
      let blocked = false;
      request.onupgradeneeded = () => {
        request.result.createObjectStore(SAVE_OBJECT_STORE);
      };
      request.onblocked = () => {
        blocked = true;
        reject(new Error("IndexedDB 存档库被其他窗口阻塞，请关闭其他游戏窗口后重试"));
      };
      request.onerror = () => reject(new Error(`打开 IndexedDB 存档库失败：${describe(request.error)}`));
      request.onsuccess = () => {
        const database = request.result;
        if (blocked) { database.close(); return; }
        database.onversionchange = () => {
          database.close();
          if (this.connection === connection) this.connection = null;
        };
        database.onclose = () => { if (this.connection === connection) this.connection = null; };
        resolve(database);
      };
    });
    const connection = pending.catch((error: unknown) => {
      if (this.connection === connection) this.connection = null;
      throw error;
    });
    this.connection = connection;
    return connection;
  }

  async getItem(key: string): Promise<string | null> {
    const database = await this.open();
    return new Promise((resolve, reject) => {
      const transaction = database.transaction(SAVE_OBJECT_STORE, "readonly");
      const request = transaction.objectStore(SAVE_OBJECT_STORE).get(key);
      let value: string | null = null;
      let invalid: Error | null = null;
      request.onsuccess = () => {
        if (request.result === undefined) value = null;
        else if (typeof request.result === "string") value = request.result;
        else {
          invalid = new Error("IndexedDB 快速槽内容必须是压缩存档字符串");
          transaction.abort();
        }
      };
      transaction.oncomplete = () => resolve(value);
      transaction.onabort = () => reject(invalid === null
        ? new Error(`读取 IndexedDB 快速槽事务中止：${describe(transaction.error)}`) : invalid);
      transaction.onerror = () => reject(new Error(`读取 IndexedDB 快速槽失败：${describe(request.error === null ? transaction.error : request.error)}`));
    });
  }

  async hasItem(key: string): Promise<boolean> {
    const database = await this.open();
    return new Promise((resolve, reject) => {
      const transaction = database.transaction(SAVE_OBJECT_STORE, "readonly");
      const request = transaction.objectStore(SAVE_OBJECT_STORE).getKey(key);
      let exists = false, failure: Error | undefined;
      request.onsuccess = () => { exists = request.result !== undefined; };
      transaction.onerror = () => { failure ??= new Error(`检查固定源失败：${describe(request.error ?? transaction.error)}`); };
      transaction.onabort = () => reject(failure ?? new Error(`检查固定源事务中止：${describe(transaction.error)}`));
      transaction.oncomplete = () => failure === undefined ? resolve(exists) : reject(failure);
    });
  }

  async setItem(key: string, value: string, isCurrent: () => boolean): Promise<boolean> {
    return this.writeItem(key, value, isCurrent, false);
  }

  async addItem(key: string, value: string, isCurrent: () => boolean, seed?: string): Promise<boolean> {
    if (seed !== undefined && (!/^(0|[1-9][0-9]*)$/.test(seed) || BigInt(seed) > 18446744073709551615n)) throw new Error("固定源 seed 无效");
    return this.writeItem(key, value, isCurrent, true, seed);
  }

  async removeItem(key: string): Promise<void> {
    const database = await this.open();
    return new Promise((resolve, reject) => {
      const transaction = database.transaction(SAVE_OBJECT_STORE, "readwrite");
      const store = transaction.objectStore(SAVE_OBJECT_STORE);
      let request: IDBRequest | undefined;
      let failure: Error | undefined;
      const id = key.startsWith(PIN_PREFIX) ? key.slice(PIN_PREFIX.length) : null;
      if (id !== null && /^[a-f0-9]{32}$/.test(id)) {
        const cursorRequest = store.openKeyCursor();
        cursorRequest.onsuccess = () => {
          const cursor = cursorRequest.result;
          if (cursor === null) { request = store.delete(key); return; }
          if (typeof cursor.key === "string" && cursor.key.startsWith(`${PIN_META_PREFIX}${id}:`)) store.delete(cursor.key);
          cursor.continue();
        };
      } else request = store.delete(key);
      transaction.onerror = () => { failure = new Error(`删除 IndexedDB 固定源失败：${describe(request?.error ?? transaction.error)}`); };
      transaction.onabort = () => reject(failure ?? new Error(`删除 IndexedDB 固定源事务中止：${describe(transaction.error)}`));
      transaction.oncomplete = () => failure === undefined ? resolve() : reject(failure);
    });
  }

  async listPinnedSources(): Promise<Array<{ sourceId: string; logicalBytes: number | null; seed: string | null }>> {
    const database = await this.open();
    return new Promise((resolve, reject) => {
      const transaction = database.transaction(SAVE_OBJECT_STORE, "readonly");
      const request = transaction.objectStore(SAVE_OBJECT_STORE).openKeyCursor();
      const rows: Array<{ sourceId: string; logicalBytes: number; seed: string | null }> = [];
      const sources = new Set<string>();
      const invalidMetadata = new Set<string>();
      let failure: Error | undefined;
      request.onsuccess = () => {
        const cursor = request.result;
        if (cursor === null) return;
        if (typeof cursor.key === "string" && cursor.key.startsWith(PIN_PREFIX)) {
          const sourceId = cursor.key.slice(PIN_PREFIX.length);
          if (!/^[a-f0-9]{32}$/.test(sourceId)) { failure = new Error("固定源记录无效，无法安全列出"); transaction.abort(); return; }
          sources.add(sourceId);
        } else if (typeof cursor.key === "string" && cursor.key.startsWith(PIN_META_PREFIX)) {
          const match = /^wasm-handover-meta:([a-f0-9]{32}):(0|[1-9][0-9]*)(?::(0|[1-9][0-9]*))?$/.exec(cursor.key);
          if (match === null || !Number.isSafeInteger(Number(match[2])) || (match[3] !== undefined && BigInt(match[3]) > 18446744073709551615n)) {
            const id = /^wasm-handover-meta:([a-f0-9]{32}):/.exec(cursor.key)?.[1];
            if (id === undefined) { failure = new Error("固定源元数据键无效，无法安全识别"); transaction.abort(); return; }
            invalidMetadata.add(id);
          } else rows.push({ sourceId: match[1]!, logicalBytes: Number(match[2]), seed: match[3] ?? null });
        }
        cursor.continue();
      };
      transaction.onerror = () => { failure ??= new Error(`列出固定源失败：${describe(request.error ?? transaction.error)}`); };
      transaction.onabort = () => reject(failure ?? new Error(`列出固定源事务中止：${describe(transaction.error)}`));
      transaction.oncomplete = () => {
        if (failure !== undefined) { reject(failure); return; }
        const ids = new Set([...sources, ...rows.map(row => row.sourceId), ...invalidMetadata]);
        resolve([...ids].sort().map(sourceId => {
          const matches = rows.filter(row => row.sourceId === sourceId);
          return { sourceId, logicalBytes: sources.has(sourceId) && matches.length === 1 && !invalidMetadata.has(sourceId) ? matches[0]!.logicalBytes : null, seed: matches.length === 1 ? matches[0]!.seed : null };
        }));
      };
    });
  }

  async deletePinnedSource(sourceId: string): Promise<void> {
    if (!/^[a-f0-9]{32}$/.test(sourceId)) throw new Error("固定源 sourceId 无效");
    await this.removeItem(`wasm-handover-source:${sourceId}`);
  }

  private async writeItem(key: string, value: string, isCurrent: () => boolean, onlyNew: boolean, seed?: string): Promise<boolean> {
    const database = await this.open();
    if (!isCurrent()) return false;
    return new Promise((resolve, reject) => {
      const transaction = database.transaction(SAVE_OBJECT_STORE, "readwrite");
      const objectStore = transaction.objectStore(SAVE_OBJECT_STORE);
      let request: IDBRequest | undefined;
      let obsolete = false;
      let failure: Error | null = null;
      const abort = (error: Error) => {
        failure = error;
        try { transaction.abort(); }
        catch (abortError) { failure = new AggregateError([error, abortError], "固定源事务中止失败"); }
      };
      const write = () => {
        if (onlyNew && key.startsWith(PIN_PREFIX)) objectStore.add("", `${PIN_META_PREFIX}${key.slice(PIN_PREFIX.length)}:${value.length * 2}${seed === undefined ? "" : `:${seed}`}`);
        request = onlyNew ? objectStore.add(value, key) : objectStore.put(value, key);
        request.onsuccess = () => {
        // 压缩、开库与 put 都可能异步等待；提交前再检查 Session generation。
        // commit 是成功写入的线性化点，abort 则保留上一份有效快速槽。
        try {
          if (!isCurrent()) {
            obsolete = true;
            transaction.abort();
          } else transaction.commit();
        } catch (error) {
          let message = `${obsolete ? "中止" : "提交"} IndexedDB 快速槽失败：${describe(error)}`;
          obsolete = false;
          try { transaction.abort(); }
          catch (abortError) { message += `；中止事务也失败：${describe(abortError)}`; }
          failure = new Error(message);
        }
        };
      };
      transaction.oncomplete = () => failure === null ? resolve(true) : reject(failure);
      transaction.onabort = () => obsolete ? resolve(false)
        : reject(failure ?? new Error(`写入 IndexedDB 快速槽事务中止：${describe(transaction.error)}`));
      transaction.onerror = () => { failure ??= new Error(`写入 IndexedDB 快速槽失败：${describe(request?.error ?? transaction.error)}`); };
      if (!onlyNew) { write(); return; }
      if (!key.startsWith(PIN_PREFIX)) { write(); return; }
      let total = value.length * 2, count = 0;
      if (total > this.maximumPinnedBytes) { abort(new Error(`固定源逻辑大小超过 ${this.maximumPinnedBytes} bytes 上限，未发布重载目标`)); return; }
      const sources = new Set<string>(), metadata = new Map<string, number>();
      const cursorRequest = objectStore.openKeyCursor();
      cursorRequest.onsuccess = () => {
        if (!isCurrent()) { obsolete = true; transaction.abort(); return; }
        const cursor = cursorRequest.result;
        if (cursor === null) {
          if (sources.size !== metadata.size || [...sources].some(id => !metadata.has(id))) { abort(new Error("固定源与元数据不一致")); return; }
          write(); return;
        }
        if (typeof cursor.key === "string" && cursor.key.startsWith(PIN_PREFIX)) {
          const id = cursor.key.slice(PIN_PREFIX.length);
          if (!/^[a-f0-9]{32}$/.test(id)) { abort(new Error("固定源键无效")); return; }
          sources.add(id);
        } else if (typeof cursor.key === "string" && cursor.key.startsWith(PIN_META_PREFIX)) {
          const match = /^wasm-handover-meta:([a-f0-9]{32}):(0|[1-9][0-9]*)(?::(0|[1-9][0-9]*))?$/.exec(cursor.key);
          if (match === null || !Number.isSafeInteger(Number(match[2])) || (match[3] !== undefined && BigInt(match[3]) > 18446744073709551615n) || metadata.has(match[1]!)) { abort(new Error("固定源元数据无效")); return; }
          metadata.set(match[1]!, Number(match[2]));
          count++;
          if (count >= 1024) { abort(new Error("固定源数量达到 1024 上限，未发布重载目标")); return; }
          total += Number(match[2]);
          if (!Number.isSafeInteger(total) || total > this.maximumPinnedBytes) { abort(new Error(`固定源逻辑大小超过 ${this.maximumPinnedBytes} bytes 上限，未发布重载目标`)); return; }
        }
        cursor.continue();
      };
      cursorRequest.onerror = () => { failure = new Error(`读取固定源总量失败：${describe(cursorRequest.error)}`); };
    });
  }
}

export class CompressedIndexedDbSaveRepository extends CompressedSaveRepository {
  constructor(factory: IDBFactory, priorStorage: Pick<Storage, "getItem">,
    codec: SaveCompressionCodec = gzipSaveCodec, database = SAVE_DATABASE) {
    const storage = new IndexedDbSaveStorage(factory, database);
    super({
      async getItem(key) {
        const current = await storage.getItem(key);
        // 仅在新库尚无快速槽时读取已有的同格式槽；坏档或开库失败仍显式报错。
        return current === null ? priorStorage.getItem(key) : current;
      },
      setItem: (key, value, isCurrent) => storage.setItem(key, value, isCurrent),
    }, QUICK_SAVE_KEY, codec);
  }
}
