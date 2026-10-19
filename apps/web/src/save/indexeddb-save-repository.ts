import type { ArchiveMetadata, BrowserArchiveStore } from "./archive-store.ts";
import { parseArchiveMetadata as metadata } from "./archive-store.ts";
import { validateDayEndArchive } from "./day-end-candidate.ts";
import { parseSaveSlot } from "./save-schema.ts";
import type { StrictSaveEnvelope } from "./schema/root.ts";

const stores = ["archives", "metadata", "selection"];

function archiveName(value: string): string {
  if (typeof value !== "string" || value.trim().length === 0) throw new Error("存档名称不能为空");
  return value.trim();
}

export class IndexedDbSaveRepository implements BrowserArchiveStore {
  private opening: Promise<IDBDatabase> | null = null;
  private currentSlotId: string | null = null;
  private selectionEpoch = 0;
  private readonly activeWrites = new Set<IDBTransaction>();
  private readonly factory: IDBFactory;
  private readonly databaseName: string;

  constructor(factory: IDBFactory, databaseName = "stock-game-day-end") {
    if (!factory) throw new Error("IndexedDB 不可用；无法保存浏览器本地游戏，请检查浏览器权限并反馈错误");
    this.factory = factory;
    this.databaseName = databaseName;
  }

  private open(): Promise<IDBDatabase> {
    this.opening ??= new Promise((resolve, reject) => {
      const request = this.factory.open(this.databaseName);
      let blocked = false;
      request.onupgradeneeded = (event) => {
        if (event.oldVersion !== 0) { request.transaction!.abort(); return; }
        request.result.createObjectStore("archives");
        request.result.createObjectStore("metadata", { keyPath: "slot_id" });
        request.result.createObjectStore("selection");
      };
      request.onerror = () => reject(new Error(`打开 IndexedDB 存档失败：${request.error?.message}`));
      request.onblocked = () => {
        blocked = true;
        reject(new Error("IndexedDB 存档被其他页面阻塞，请关闭其他游戏页面后重试"));
      };
      request.onsuccess = () => {
        const database = request.result;
        if (blocked) { database.close(); return; }
        if (Array.from(database.objectStoreNames).sort().join() !== stores.join() || database.version !== 1) {
          database.close(); reject(new Error("IndexedDB 数据库结构无效；不支持旧结构或迁移，请明确删除旧数据库或选择其他存档")); return;
        }
        const transaction = database.transaction(stores, "readonly");
        const valid = stores.every((name) => {
            const store = transaction.objectStore(name);
            return store.keyPath === (name === "metadata" ? "slot_id" : null)
              && !store.autoIncrement && store.indexNames.length === 0;
          });
        if (!valid) { database.close(); reject(new Error("IndexedDB 数据库结构无效；不支持旧结构或迁移，请明确删除旧数据库或选择其他存档")); return; }
        database.onversionchange = () => { database.close(); this.opening = null; };
        database.onclose = () => { this.opening = null; };
        resolve(database);
      };
    });
    return this.opening;
  }

  private async transact<Value>(mode: IDBTransactionMode, operation: (transaction: IDBTransaction, done: (value: Value) => void, fail: (error: unknown) => void) => void): Promise<Value> {
    const database = await this.open();
    return new Promise((resolve, reject) => {
      const transaction = database.transaction(stores, mode);
      let result: Value;
      let completed = false;
      let failure: unknown;
      if (mode === "readwrite") this.activeWrites.add(transaction);
      const fail = (error: unknown) => { failure = error; transaction.abort(); };
      transaction.oncomplete = () => {
        this.activeWrites.delete(transaction);
        if (!completed) { reject(new Error("IndexedDB 事务未返回结果；请反馈错误")); return; }
        resolve(result);
      };
      transaction.onabort = () => {
        this.activeWrites.delete(transaction);
        reject(failure ?? new Error(`IndexedDB 存档事务已取消：${transaction.error?.message ?? "会话已替换"}`));
      };
      transaction.onerror = (event) => {
        if (failure === undefined) {
          const request = event.target as IDBRequest;
          failure = new Error(`IndexedDB 存档事务失败：${request.error?.message ?? transaction.error?.message ?? "未知存储错误，请反馈"}`);
        }
      };
      try { operation(transaction, (value) => { result = value; completed = true; }, fail); }
      catch (error) { fail(error); }
    });
  }

  cancelPending(): void {
    for (const transaction of this.activeWrites) transaction.abort();
  }

  newSlot(): void {
    this.cancelPending();
    this.selectionEpoch += 1;
    this.currentSlotId = crypto.randomUUID();
  }

  async select(slotId: string, isCurrent: () => boolean = () => true): Promise<boolean> {
    if (!isCurrent()) return false;
    const previous = this.currentSlotId;
    const epoch = ++this.selectionEpoch;
    const current = () => this.selectionEpoch === epoch && isCurrent();
    const stale = Symbol("旧代槽位选择");
    this.currentSlotId = slotId;
    try {
      const committed = await this.transact<boolean>("readwrite", (transaction, done, fail) => {
        if (!current()) { done(false); return; }
        const request = transaction.objectStore("metadata").get(slotId);
        request.onsuccess = () => {
          try {
            if (!current()) { done(false); return; }
            if (request.result === undefined) throw new Error(`存档槽 ${slotId} 不存在`);
            metadata(request.result);
            const selection = transaction.objectStore("selection").put(slotId, "current");
            selection.onsuccess = () => { if (!current()) { fail(stale); return; } done(true); };
          } catch (error) { fail(error); }
        };
      });
      if (!committed && this.selectionEpoch === epoch) this.currentSlotId = previous;
      return committed && current();
    } catch (error) {
      if (error === stale) { if (this.selectionEpoch === epoch) this.currentSlotId = previous; return false; }
      throw error;
    }
  }

  async save(value: unknown, isCurrent: () => boolean = () => true): Promise<boolean> {
    const epoch = this.selectionEpoch;
    const current = () => this.selectionEpoch === epoch && isCurrent();
    const slot = validateDayEndArchive(parseSaveSlot(value));
    if (!current()) return false;
    const slotId = this.currentSlotId ?? crypto.randomUUID();
    const committed = await this.transact<boolean>("readwrite", (transaction, done, fail) => {
      if (!current()) { done(false); return; }
      const request = transaction.objectStore("metadata").get(slotId);
      request.onsuccess = () => {
        if (!current()) { done(false); return; }
        try {
          const name = request.result === undefined ? "自动日终存档" : metadata(request.result).name;
          const entry: ArchiveMetadata = { slot_id: slotId, name, civil_date: slot.civil_clock.settled_through!, tick: slot.snapshot.tick };
          transaction.objectStore("archives").put(slot, slotId);
          transaction.objectStore("metadata").put(entry);
          const selection = transaction.objectStore("selection").put(slotId, "current");
          selection.onsuccess = () => { if (!current()) { fail(new Error("旧会话日终写入已取消")); return; } done(true); };
        } catch (error) { fail(error); }
      };
    });
    if (committed && current()) this.currentSlotId = slotId;
    return committed && current();
  }

  list(): Promise<ArchiveMetadata[]> {
    return this.transact("readonly", (transaction, done, fail) => {
      const request = transaction.objectStore("metadata").getAll();
      request.onsuccess = () => {
        try { done(request.result.map(metadata).sort((left, right) => left.slot_id.localeCompare(right.slot_id))); }
        catch (error) { fail(error); }
      };
    });
  }

  async load(slotId?: string): Promise<StrictSaveEnvelope | null> {
    const epoch = this.selectionEpoch;
    const loaded = await this.transact<{ slot: StrictSaveEnvelope | null; slotId: string | null }>("readonly", (transaction, done, fail) => {
      const read = (selected: unknown) => {
        if (selected === undefined) { done({ slot: null, slotId: null }); return; }
        if (typeof selected !== "string" || selected.length === 0) { fail(new Error("IndexedDB 当前槽位结构无效")); return; }
        const request = transaction.objectStore("archives").get(selected);
        request.onsuccess = () => {
          try {
            if (request.result === undefined) throw new Error(`当前存档槽 ${selected} 不存在`);
            const slot = validateDayEndArchive(parseSaveSlot(request.result));
            const information = transaction.objectStore("metadata").get(selected);
            information.onsuccess = () => {
              try {
                const entry = metadata(information.result);
                if (entry.slot_id !== selected || entry.civil_date !== slot.civil_clock.settled_through || entry.tick !== slot.snapshot.tick) throw new Error("IndexedDB 日终档与元数据不一致，请反馈错误");
                done({ slot, slotId: selected });
              } catch (error) { fail(error); }
            };
          } catch (error) { fail(error); }
        };
      };
      if (slotId !== undefined) read(slotId);
      else { const request = transaction.objectStore("selection").get("current"); request.onsuccess = () => read(request.result); }
    });
    if (slotId === undefined && this.selectionEpoch === epoch) this.currentSlotId = loaded.slotId;
    return loaded.slot;
  }

  rename(slotId: string, name: string): Promise<void> {
    const validatedName = archiveName(name);
    return this.transact("readwrite", (transaction, done, fail) => {
      const request = transaction.objectStore("metadata").get(slotId);
      request.onsuccess = () => {
        try {
          if (request.result === undefined) throw new Error(`存档槽 ${slotId} 不存在`);
          transaction.objectStore("metadata").put({ ...metadata(request.result), name: validatedName });
          done();
        } catch (error) { fail(error); }
      };
    });
  }

  async delete(slotId: string): Promise<void> {
    await this.transact<void>("readwrite", (transaction, done, fail) => {
      const request = transaction.objectStore("metadata").get(slotId);
      request.onsuccess = () => {
        if (request.result === undefined) { fail(new Error(`存档槽 ${slotId} 不存在`)); return; }
        transaction.objectStore("archives").delete(slotId);
        transaction.objectStore("metadata").delete(slotId);
        const selected = transaction.objectStore("selection").get("current");
        selected.onsuccess = () => { if (selected.result === slotId) transaction.objectStore("selection").delete("current"); done(); };
      };
    });
    if (this.currentSlotId === slotId) { this.selectionEpoch += 1; this.currentSlotId = crypto.randomUUID(); }
  }

  copy(slotId: string, name: string): Promise<ArchiveMetadata> {
    const validatedName = archiveName(name);
    return this.transact("readwrite", (transaction, done, fail) => {
      const request = transaction.objectStore("archives").get(slotId);
      request.onsuccess = () => {
        try {
          if (request.result === undefined) throw new Error(`存档槽 ${slotId} 不存在`);
          const slot = validateDayEndArchive(parseSaveSlot(request.result));
          const entry: ArchiveMetadata = { slot_id: crypto.randomUUID(), name: validatedName, civil_date: slot.civil_clock.settled_through!, tick: slot.snapshot.tick };
          transaction.objectStore("archives").put(slot, entry.slot_id);
          transaction.objectStore("metadata").put(entry);
          done(entry);
        } catch (error) { fail(error); }
      };
    });
  }

  async close(): Promise<void> {
    this.cancelPending();
    if (this.opening !== null) (await this.opening).close();
    this.opening = null;
  }
}
