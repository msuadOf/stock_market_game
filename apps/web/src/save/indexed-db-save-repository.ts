import { CompressedSaveRepository, gzipSaveCodec, type AsyncSaveStorage, type SaveCompressionCodec } from "./save-repository.ts";

import { SAVE_DATABASE, SAVE_OBJECT_STORE, QUICK_SAVE_KEY } from "./save-storage-keys.ts";
export { SAVE_DATABASE, SAVE_OBJECT_STORE, QUICK_SAVE_KEY } from "./save-storage-keys.ts";

function describe(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export class IndexedDbSaveStorage implements AsyncSaveStorage {
  private connection: Promise<IDBDatabase> | null = null;
  private readonly factory: IDBFactory;
  private readonly database: string;

  constructor(factory: IDBFactory, database = SAVE_DATABASE) {
    if (factory === undefined || factory === null) throw new Error("当前浏览器不支持 IndexedDB，无法保存本地快速槽");
    this.factory = factory;
    this.database = database;
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

  async setItem(key: string, value: string, isCurrent: () => boolean): Promise<boolean> {
    const database = await this.open();
    if (!isCurrent()) return false;
    return new Promise((resolve, reject) => {
      const transaction = database.transaction(SAVE_OBJECT_STORE, "readwrite");
      const request = transaction.objectStore(SAVE_OBJECT_STORE).put(value, key);
      let obsolete = false;
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
          reject(new Error(message));
        }
      };
      transaction.oncomplete = () => resolve(true);
      transaction.onabort = () => obsolete ? resolve(false)
        : reject(new Error(`写入 IndexedDB 快速槽事务中止：${describe(transaction.error)}`));
      transaction.onerror = () => reject(new Error(`写入 IndexedDB 快速槽失败：${describe(request.error === null ? transaction.error : request.error)}`));
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
