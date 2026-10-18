import assert from "node:assert/strict";
import test from "node:test";
import { CompressedIndexedDbSaveRepository, IndexedDbSaveStorage, QUICK_SAVE_KEY } from "./indexed-db-save-repository.ts";
import { currentSaveFixture } from "./current-save-fixture.ts";
import { parseSaveSlot } from "./save-schema.ts";

interface FakeRequest {
  result: unknown;
  onsuccess: (() => void) | null;
  onerror: (() => void) | null;
  onabort: (() => void) | null;
  oncomplete: (() => void) | null;
  error: Error | null;
}

function fakeFactory(options: {
  beforeOpen?: () => void;
  beforePut?: () => void;
  putError?: Error;
  commitError?: Error;
} = {}) {
  const entries = new Map<string, unknown>();
  let commits = 0;
  let opens = 0;
  let closed = false;
  function request(result: unknown): FakeRequest {
    return { result, onsuccess: null, onerror: null, onabort: null, oncomplete: null, error: null };
  }
  const database = {
    onclose: null as (() => void) | null,
    close() { closed = true; },
    transaction() {
      if (closed) throw new Error("数据库连接已关闭");
      const transaction = request(undefined);
      let aborted = false;
      let write: { key: string; value: string } | null = null;
      return Object.assign(transaction, {
        abort() {
          aborted = true;
          queueMicrotask(() => transaction.onabort?.());
        },
        commit() {
          if (options.commitError) throw options.commitError;
          if (write === null) throw new Error("测试事务缺少 put");
          entries.set(write.key, write.value);
          commits += 1;
          queueMicrotask(() => transaction.oncomplete?.());
        },
        objectStore() {
          return {
            get(key: string) {
              const read = request(entries.get(key));
              queueMicrotask(() => {
                read.onsuccess?.();
                if (!aborted) transaction.oncomplete?.();
              });
              return read;
            },
            put(value: string, key: string) {
              write = { key, value };
              const put = request(key);
              queueMicrotask(() => {
                options.beforePut?.();
                if (options.putError) {
                  transaction.error = options.putError;
                  transaction.onerror?.();
                  transaction.onabort?.();
                } else put.onsuccess?.();
              });
              return put;
            },
          };
        },
      });
    },
  };
  const factory = {
    open() {
      opens += 1;
      const opening = request(database);
      queueMicrotask(() => { closed = false; options.beforeOpen?.(); opening.onsuccess?.(); });
      return opening;
    },
  } as unknown as IDBFactory;
  return { factory, entries, commits: () => commits, opens: () => opens,
    unexpectedClose() { closed = true; database.onclose?.(); } };
}

const slot = parseSaveSlot(currentSaveFixture());
const encoded = `gzip:${JSON.stringify(slot)}`;
const codec = { encode: async (text: string) => text, decode: async (text: string) => text };

test("快速槽事务写入 IndexedDB，LocalStorage 容量错误不能影响保存和恢复", async () => {
  const fake = fakeFactory();
  const repository = new CompressedIndexedDbSaveRepository(fake.factory, {
    getItem: () => { throw new Error("不应读取旧槽"); },
  }, codec);
  assert.equal(await repository.save(slot), true);
  assert.equal(fake.commits(), 1);
  assert.equal(fake.entries.get(QUICK_SAVE_KEY), encoded);
  assert.deepEqual(await repository.load(), slot);
});

test("新库无槽时只读已有同格式 LocalStorage 档，不在启动时复制写盘", async () => {
  const fake = fakeFactory();
  let reads = 0;
  const repository = new CompressedIndexedDbSaveRepository(fake.factory, {
    getItem: () => { reads += 1; return encoded; },
  }, codec);
  assert.deepEqual(await repository.load(), slot);
  assert.equal(reads, 1);
  assert.equal(fake.entries.size, 0);
  assert.equal(fake.commits(), 0);
});

test("IndexedDB 坏档不会退回旧 LocalStorage 档掩盖损坏", async () => {
  const fake = fakeFactory();
  fake.entries.set(QUICK_SAVE_KEY, "bad archive");
  const repository = new CompressedIndexedDbSaveRepository(fake.factory, {
    getItem: () => { throw new Error("坏档不能回退旧槽"); },
  }, codec);
  await assert.rejects(repository.load(), /缺少 gzip:/);
  fake.entries.set(QUICK_SAVE_KEY, 42);
  await assert.rejects(repository.load(), /快速槽内容必须是压缩存档字符串/);
});

test("等待开库期间变更 Session generation 不启动写事务", async () => {
  let current = true;
  const fake = fakeFactory({ beforeOpen: () => { current = false; } });
  fake.entries.set(QUICK_SAVE_KEY, "previous");
  const storage = new IndexedDbSaveStorage(fake.factory);
  assert.equal(await storage.setItem(QUICK_SAVE_KEY, "new", () => current), false);
  assert.equal(fake.entries.get(QUICK_SAVE_KEY), "previous");
  assert.equal(fake.commits(), 0);
});

test("put 完成前旧 Session 失效时 abort 事务，保留原有效快速槽", async () => {
  let current = true;
  const fake = fakeFactory({ beforePut: () => { current = false; } });
  fake.entries.set(QUICK_SAVE_KEY, "previous");
  const storage = new IndexedDbSaveStorage(fake.factory);
  assert.equal(await storage.setItem(QUICK_SAVE_KEY, "new", () => current), false);
  assert.equal(fake.entries.get(QUICK_SAVE_KEY), "previous");
  assert.equal(fake.commits(), 0);
});

test("IndexedDB quota 和 commit 失败均显式报错，保留原有效快速槽", async () => {
  for (const options of [{ putError: new Error("IndexedDB quota") }, { commitError: new Error("commit unavailable") }]) {
    const fake = fakeFactory(options);
    fake.entries.set(QUICK_SAVE_KEY, "previous");
    const storage = new IndexedDbSaveStorage(fake.factory);
    await assert.rejects(storage.setItem(QUICK_SAVE_KEY, "new", () => true), /IndexedDB quota|commit unavailable/);
    assert.equal(fake.entries.get(QUICK_SAVE_KEY), "previous");
    assert.equal(fake.commits(), 0);
  }
});

test("数据库连接意外关闭后下一次显式操作重新开库", async () => {
  const fake = fakeFactory();
  const storage = new IndexedDbSaveStorage(fake.factory);
  assert.equal(await storage.setItem(QUICK_SAVE_KEY, "previous", () => true), true);
  fake.unexpectedClose();
  assert.equal(await storage.getItem(QUICK_SAVE_KEY), "previous");
  assert.equal(fake.opens(), 2);
  assert.equal(await storage.setItem(QUICK_SAVE_KEY, "new", () => true), true);
  assert.equal(await storage.getItem(QUICK_SAVE_KEY), "new");
});
