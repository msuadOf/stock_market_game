import assert from "node:assert/strict";
import test from "node:test";
import { IndexedDbCredentialStore, parseStoredCredential } from "./credential-store.ts";

const stored = { server: "https://example.test", token: "b".repeat(64), subject: { subject_id: "guest", username: null } };

test("凭据独立于市场，严格拒绝 password、错 server、坏 token 且错误不回显秘密", { timeout: 10000 }, () => {
  assert.deepEqual(parseStoredCredential(stored, stored.server), stored);
  for (const candidate of [{ ...stored, password: "do-not-echo" }, { ...stored, server: "https://another.test" }, { ...stored, token: "do-not-echo" }]) {
    assert.throws(() => parseStoredCredential(candidate, stored.server), (error: Error) => !error.message.includes("do-not-echo"));
  }
  assert.throws(() => new IndexedDbCredentialStore(undefined), /IndexedDB/);
});

function memoryFactory() {
  const records = new Map<string, unknown>();
  const opened: string[] = [];
  let failWrite = false;
  let failOpen = false;
  let finished = false;
  const store = { keyPath: null, autoIncrement: false, indexNames: [],
    get(key: string) {
      const request = {} as IDBRequest;
      queueMicrotask(() => { Object.assign(request, { result: records.get(key) }); request.onsuccess?.({} as Event); });
      return request;
    },
    put(value: unknown, key: string) { if (failWrite) throw new Error("storage secret must not echo"); records.set(key, structuredClone(value)); },
    delete(key: string) { records.delete(key); },
  };
  const database = { version: 1, objectStoreNames: ["credentials"], close() {}, createObjectStore() {},
    transaction() {
      const transaction = { objectStore: () => store, abort() { if (finished) throw new DOMException("事务已结束", "InvalidStateError"); finished = true; queueMicrotask(() => transaction.onabort?.()); }, onabort: null as null | (() => void), oncomplete: null as null | (() => void), onerror: null };
      setTimeout(() => transaction.oncomplete?.(), 0);
      return transaction;
    },
  };
  const factory = { open(name: string) {
    opened.push(name);
    const request = { result: database } as unknown as IDBOpenDBRequest;
    queueMicrotask(() => failOpen ? request.onerror?.({} as Event) : request.onsuccess?.({} as Event));
    return request;
  } } as unknown as IDBFactory;
  return { factory, records, opened, fail: () => { failWrite = true; }, failOpening: (value: boolean) => { failOpen = value; }, finishTransaction: () => { finished = true; } };
}

test("IndexedDB 使用独立 auth DB 且按 Server 隔离，事务错误显式失败", { timeout: 10000 }, async () => {
  const memory = memoryFactory();
  const repository = new IndexedDbCredentialStore(memory.factory);
  assert.equal(await repository.load(stored.server), null);
  await repository.save(stored);
  assert.deepEqual(await repository.load(`${stored.server}/`), stored);
  assert.equal(await repository.load("https://another.test"), null);
  assert.deepEqual(memory.opened, ["stock-game-auth"]);
  await repository.remove(stored.server);
  assert.equal(await repository.load(stored.server), null);
  memory.fail();
  await assert.rejects(repository.save(stored), (error: Error) => error.message.includes("IndexedDB") && !error.message.includes("storage secret"));
});

test("已取消页面的延迟 open 不得写入或删除新页面 credential", { timeout: 10000 }, async () => {
  const memory = memoryFactory();
  const repository = new IndexedDbCredentialStore(memory.factory);
  let current = true;
  const staleSave = repository.save(stored, () => current);
  current = false;
  repository.cancelPending();
  await assert.rejects(staleSave, /取消/);
  assert.equal(memory.records.size, 0);
  const replacement = { ...stored, token: "c".repeat(64) };
  await repository.save(replacement);
  await assert.rejects(repository.remove(stored.server, () => false), /取消/);
  assert.deepEqual(await repository.load(stored.server), replacement);
});

test("打开失败后同一 repository 可显式重试；重复取消不阻断返回", { timeout: 10000 }, async () => {
  const memory = memoryFactory();
  const repository = new IndexedDbCredentialStore(memory.factory);
  memory.failOpening(true);
  await assert.rejects(repository.load(stored.server), /无法打开/);
  memory.failOpening(false);
  assert.equal(await repository.load(stored.server), null);
  const pending = repository.save(stored);
  const checked = assert.rejects(pending, /取消/);
  await Promise.resolve();
  await Promise.resolve();
  repository.cancelPending();
  assert.doesNotThrow(() => repository.cancelPending());
  await checked;
  const finished = repository.save(stored);
  await Promise.resolve();
  await Promise.resolve();
  memory.finishTransaction();
  assert.doesNotThrow(() => repository.cancelPending());
  await finished;
});
