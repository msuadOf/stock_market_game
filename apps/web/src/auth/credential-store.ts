import { credentialServerKey, parseCredentialToken, parseIdentitySubject, type RemoteLogin } from "../host/remote-auth.ts";

export interface StoredCredential extends RemoteLogin { readonly server: string }

export function parseStoredCredential(value: unknown, server: string): StoredCredential {
  try {
    if (value === null || typeof value !== "object" || Array.isArray(value)) throw new Error();
    const source = value as Record<string, unknown>;
    if (Object.keys(source).length !== 3 || !["server", "token", "subject"].every((key) => Object.hasOwn(source, key)) || source.server !== credentialServerKey(server)) throw new Error();
    return { server: source.server as string, token: parseCredentialToken(source.token), subject: parseIdentitySubject(source.subject) };
  } catch { throw new Error("IndexedDB 登录凭据结构或 Server 范围无效；请明确清除该 Server 凭据并重新登录，反馈 auth.credential-store"); }
}

export class IndexedDbCredentialStore {
  private readonly factory: IDBFactory;
  private opening: Promise<IDBDatabase> | null = null;
  private readonly activeWrites = new Set<IDBTransaction>();
  constructor(factory: IDBFactory | undefined = globalThis.indexedDB) {
    if (factory === undefined) throw new Error("IndexedDB 不可用；无法记住登录，请检查浏览器权限或取消记住登录并反馈 auth.credential-store");
    this.factory = factory;
  }

  private open(): Promise<IDBDatabase> {
    if (this.opening !== null) return this.opening;
    this.opening = new Promise<IDBDatabase>((resolve, reject) => {
      const request = this.factory.open("stock-game-auth");
      let blocked = false;
      request.onupgradeneeded = (event) => {
        if (event.oldVersion !== 0) { request.transaction!.abort(); return; }
        request.result.createObjectStore("credentials");
      };
      request.onerror = () => reject(new Error("打开 IndexedDB 登录凭据失败；请检查浏览器权限并反馈 auth.credential-store"));
      request.onblocked = () => { blocked = true; reject(new Error("IndexedDB 登录凭据被其他页面阻塞，请关闭其他游戏页面后重试")); };
      request.onsuccess = () => {
        const database = request.result;
        if (blocked) { database.close(); return; }
        if (database.version !== 1 || Array.from(database.objectStoreNames).join() !== "credentials") {
          database.close(); reject(new Error("IndexedDB 登录数据库结构无效，不支持旧结构或迁移；请明确清理登录数据库")); return;
        }
        const store = database.transaction("credentials", "readonly").objectStore("credentials");
        if (store.keyPath !== null || store.autoIncrement || store.indexNames.length !== 0) {
          database.close(); reject(new Error("IndexedDB 登录数据库 credentials 结构无效，请明确清理登录数据库")); return;
        }
        database.onversionchange = () => { database.close(); this.opening = null; };
        resolve(database);
      };
    }).catch((error: unknown) => { this.opening = null; throw error; });
    return this.opening;
  }

  cancelPending(): void {
    for (const transaction of this.activeWrites) {
      this.activeWrites.delete(transaction);
      try { transaction.abort(); }
      catch (error) {
        if (!(error instanceof DOMException) || error.name !== "InvalidStateError") throw error;
      }
    }
  }

  private async transact<Value>(mode: IDBTransactionMode, operation: (store: IDBObjectStore, done: (value: Value) => void, fail: () => void) => void, isCurrent: () => boolean = () => true): Promise<Value> {
    let database: IDBDatabase;
    try { database = await this.open(); } catch { throw new Error("IndexedDB 登录数据库无法打开；请检查权限、关闭阻塞页面并反馈 auth.credential-store"); }
    if (!isCurrent()) throw new Error("旧页面 IndexedDB 登录操作已取消");
    return new Promise((resolve, reject) => {
      let transaction: IDBTransaction;
      try { transaction = database.transaction("credentials", mode); } catch { reject(new Error("IndexedDB 登录事务无法建立，请反馈 auth.credential-store")); return; }
      if (mode === "readwrite") this.activeWrites.add(transaction);
      let result: Value;
      let ready = false;
      let failed = false;
      const fail = () => { failed = true; transaction.abort(); };
      transaction.oncomplete = () => {
        this.activeWrites.delete(transaction);
        if (failed || !ready) reject(new Error("IndexedDB 登录事务未完成，请反馈 auth.credential-store"));
        else resolve(result);
      };
      transaction.onabort = () => {
        this.activeWrites.delete(transaction);
        reject(new Error("IndexedDB 登录事务失败或已取消，凭据未确认保存／删除；请检查浏览器权限并反馈 auth.credential-store"));
      };
      transaction.onerror = () => { failed = true; };
      try { operation(transaction.objectStore("credentials"), (value) => { if (!isCurrent()) { fail(); return; } result = value; ready = true; }, fail); } catch { fail(); }
    });
  }

  async load(server: string): Promise<StoredCredential | null> {
    const key = credentialServerKey(server);
    const value = await this.transact<unknown>("readonly", (store, done) => {
      const request = store.get(key);
      request.onsuccess = () => done(request.result);
    });
    return value === undefined ? null : parseStoredCredential(value, key);
  }
  async save(value: StoredCredential, isCurrent: () => boolean = () => true): Promise<void> {
    const parsed = parseStoredCredential(value, credentialServerKey(value.server));
    await this.transact<void>("readwrite", (store, done) => { store.put(parsed, parsed.server); done(); }, isCurrent);
  }
  async remove(server: string, isCurrent: () => boolean = () => true): Promise<void> {
    const key = credentialServerKey(server);
    await this.transact<void>("readwrite", (store, done) => { store.delete(key); done(); }, isCurrent);
  }
}
