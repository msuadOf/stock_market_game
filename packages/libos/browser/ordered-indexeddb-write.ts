// 通用 IndexedDB 单对象仓库的原子读改写；调用方决定索引和值。
export type IndexedUpdate = null | readonly (readonly [IDBValidKey, unknown])[];

export class OrderedIndexedDbWrite {
  private connection: Promise<IDBDatabase> | null = null;
  private readonly factory: IDBFactory;
  private readonly name: string;
  private readonly storeName: string;
  constructor(factory: IDBFactory, name: string, storeName: string) {
    this.factory = factory;
    this.name = name;
    this.storeName = storeName;
  }

  private open(): Promise<IDBDatabase> {
    if (this.connection !== null) return this.connection;
    const promise = new Promise<IDBDatabase>((resolve, reject) => {
      const request = this.factory.open(this.name, 1);
      let blocked = false;
      request.onupgradeneeded = () => { request.result.createObjectStore(this.storeName); };
      request.onerror = () => reject(request.error ?? new Error("IndexedDB 打开失败"));
      request.onblocked = () => { blocked = true; reject(new Error("IndexedDB 打开被阻塞")); };
      request.onsuccess = () => {
        const database = request.result;
        if (blocked) { database.close(); return; }
        database.onversionchange = () => {
          database.close();
          if (this.connection === promise) this.connection = null;
        };
        database.onclose = () => { if (this.connection === promise) this.connection = null; };
        resolve(database);
      };
    });
    this.connection = promise;
    void promise.catch(() => { if (this.connection === promise) this.connection = null; });
    return promise;
  }

  async change(key: IDBValidKey, decide: (previous: unknown) => IndexedUpdate): Promise<boolean> {
    const database = await this.open();
    return new Promise((resolve, reject) => {
      const transaction = database.transaction(this.storeName, "readwrite");
      const store = transaction.objectStore(this.storeName);
      let applied = false;
      let failure: unknown = null;
      const request = store.get(key);
      request.onsuccess = () => {
        try {
          const writes = decide(request.result);
          if (writes === null) return;
          for (const [nextKey, value] of writes) store.put(value, nextKey);
          applied = true;
        } catch (error) {
          failure = error;
          transaction.abort();
        }
      };
      transaction.oncomplete = () => resolve(applied);
      transaction.onabort = () => reject(failure ?? transaction.error ?? new Error("IndexedDB 更新事务已中止"));
    });
  }

  async close(): Promise<void> {
    const connection = this.connection;
    this.connection = null;
    if (connection !== null) (await connection).close();
  }
}
