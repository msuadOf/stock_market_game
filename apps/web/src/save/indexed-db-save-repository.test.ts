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
  holdAbort?: (finish: () => void) => void;
  rejectValueCursor?: boolean;
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
    transaction(_store?: unknown, mode?: string) {
      if (closed) throw new Error("数据库连接已关闭");
      const transaction = request(undefined);
      let aborted = false;
      const writes: Array<{ key: string; value: string }> = [];
      let pendingDeletes = 0;
      let scans = 0;
      return Object.assign(transaction, {
        abort() {
          aborted = true;
          queueMicrotask(() => transaction.onabort?.());
        },
        commit() {
          if (options.commitError) throw options.commitError;
          if (writes.length === 0) throw new Error("测试事务缺少 put");
          for (const write of writes) entries.set(write.key, write.value);
          commits += 1;
          queueMicrotask(() => transaction.oncomplete?.());
        },
        objectStore() {
          return {
            openCursor() {
              if (options.rejectValueCursor) throw new Error("不得扫描存档载荷");
              const scan = request(null), rows = [...entries.entries()]; let index = 0;
              const next = () => { const row = rows[index++]; scan.result = row === undefined ? null : { key: row[0], value: row[1], continue: () => queueMicrotask(next) }; scan.onsuccess?.(); if (row === undefined && mode === "readonly" && !aborted) transaction.oncomplete?.(); };
              queueMicrotask(next); return scan;
            },
            openKeyCursor() {
              const scan = request(null), rows = [...entries.keys()]; let index = 0; scans++;
              const next = () => { const key = rows[index++]; scan.result = key === undefined ? null : { key, continue: () => queueMicrotask(next) }; scan.onsuccess?.(); if (key === undefined) { scans--; if (mode === "readonly" && !aborted) transaction.oncomplete?.(); } };
              queueMicrotask(next); return scan;
            },
            get(key: string) {
              const read = request(entries.get(key));
              queueMicrotask(() => {
                read.onsuccess?.();
                if (!aborted) transaction.oncomplete?.();
              });
              return read;
            },
            put(value: string, key: string) {
              writes.push({ key, value });
              const put = request(key);
              queueMicrotask(() => {
                options.beforePut?.();
                if (options.putError) {
                  transaction.error = options.putError;
                  transaction.onerror?.();
                  if (options.holdAbort) options.holdAbort(() => transaction.onabort?.());
                  else transaction.onabort?.();
                } else put.onsuccess?.();
              });
              return put;
            },
            add(value: string, key: string) {
              if (!entries.has(key)) return this.put(value, key);
              const failed = request(undefined);
              queueMicrotask(() => { transaction.error = new Error("ConstraintError"); transaction.onerror?.(); transaction.onabort?.(); });
              return failed;
            },
            delete(key: string) {
              const removed = request(undefined);
              pendingDeletes++;
              queueMicrotask(() => { entries.delete(key); pendingDeletes--; if (pendingDeletes === 0 && scans === 0) transaction.oncomplete?.(); });
              return removed;
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

test("写入错误后等待事务真正中止才结束", { timeout: 10000 }, async () => {
  let finish: (() => void) | undefined, settled = false;
  const fake = fakeFactory({ putError: new Error("quota"), holdAbort: done => { finish = done; } });
  const outcome = new IndexedDbSaveStorage(fake.factory).setItem("pinned", "value", () => true).then(() => { settled = true; }, () => { settled = true; });
  await new Promise<void>(resolve => setImmediate(resolve));
  assert.ok(finish);
  assert.equal(settled, false);
  finish(); await outcome;
  assert.equal(settled, true);
});

test("固定源 key 冲突不能覆盖已有目标", { timeout: 10000 }, async () => {
  const fake = fakeFactory(); fake.entries.set("pinned", "old");
  const storage = new IndexedDbSaveStorage(fake.factory);
  await assert.rejects(storage.addItem("pinned", "new", () => true), /ConstraintError/);
  assert.equal(fake.entries.get("pinned"), "old");
});

test("固定源总量超限时拒绝新写入并保留快速槽和旧目标", { timeout: 10000 }, async () => {
  const fake = fakeFactory(), id = "a".repeat(32); fake.entries.set(`wasm-handover-source:${id}`, "abcd"); fake.entries.set(`wasm-handover-meta:${id}:8`, ""); fake.entries.set(QUICK_SAVE_KEY, "quick");
  const storage = new IndexedDbSaveStorage(fake.factory, "test", 10);
  await assert.rejects(storage.addItem("wasm-handover-source:b", "xx", () => true), /固定源.*上限/);
  assert.equal(fake.entries.get(`wasm-handover-source:${id}`), "abcd"); assert.equal(fake.entries.get(QUICK_SAVE_KEY), "quick");
  assert.equal(fake.entries.has("wasm-handover-source:b"), false);
});

test("固定源数量达到上限时拒绝新写入", { timeout: 10000 }, async () => {
  const fake = fakeFactory();
  for (let i = 0; i < 1024; i++) { const id = i.toString(16).padStart(32, "0"); fake.entries.set(`wasm-handover-source:${id}`, ""); fake.entries.set(`wasm-handover-meta:${id}:0`, ""); }
  const storage = new IndexedDbSaveStorage(fake.factory);
  await assert.rejects(storage.addItem("wasm-handover-source:new", "x", () => true), /固定源.*上限/);
  assert.equal(fake.entries.size, 2048);
});

test("清理未发布源只删除指定 key", { timeout: 10000 }, async () => {
  const fake = fakeFactory(); fake.entries.set("pinned", "old"); fake.entries.set(QUICK_SAVE_KEY, "quick");
  await new IndexedDbSaveStorage(fake.factory).removeItem("pinned");
  assert.equal(fake.entries.has("pinned"), false); assert.equal(fake.entries.get(QUICK_SAVE_KEY), "quick");
});

test("管理固定源只列出源并仅删除用户选择的目标", { timeout: 10000 }, async () => {
  const fake = fakeFactory(), a = "a".repeat(32), b = "b".repeat(32);
  fake.entries.set(`wasm-handover-source:${a}`, "abc"); fake.entries.set(`wasm-handover-meta:${a}:6`, ""); fake.entries.set(`wasm-handover-source:${b}`, "defg"); fake.entries.set(`wasm-handover-meta:${b}:8`, ""); fake.entries.set(QUICK_SAVE_KEY, "quick");
  const storage = new IndexedDbSaveStorage(fake.factory);
  assert.deepEqual(await storage.listPinnedSources(), [{ sourceId: a, logicalBytes: 6, seed: null }, { sourceId: b, logicalBytes: 8, seed: null }]);
  await storage.deletePinnedSource(a);
  assert.equal(fake.entries.has(`wasm-handover-source:${a}`), false);
  assert.equal(fake.entries.get(`wasm-handover-source:${b}`), "defg"); assert.equal(fake.entries.get(QUICK_SAVE_KEY), "quick");
  await assert.rejects(storage.deletePinnedSource(QUICK_SAVE_KEY), /sourceId/);
});

test("固定源预算及管理只读取键与小型元数据", { timeout: 10000 }, async () => {
  const fake = fakeFactory({ rejectValueCursor: true });
  const storage = new IndexedDbSaveStorage(fake.factory);
  const id = "c".repeat(32);
  assert.equal(await storage.addItem(`wasm-handover-source:${id}`, "payload", () => true), true);
  assert.deepEqual(await storage.listPinnedSources(), [{ sourceId: id, logicalBytes: 14, seed: null }]);
});

test("缺失元数据的源可明确识别和删除，但不能绕过容量预算", { timeout: 10000 }, async () => {
  const fake = fakeFactory(), id = "e".repeat(32);
  fake.entries.set(`wasm-handover-source:${id}`, "old");
  const storage = new IndexedDbSaveStorage(fake.factory);
  assert.deepEqual(await storage.listPinnedSources(), [{ sourceId: id, logicalBytes: null, seed: null }]);
  await assert.rejects(storage.addItem(`wasm-handover-source:${"f".repeat(32)}`, "new", () => true), /元数据不一致/);
  await storage.deletePinnedSource(id);
  assert.deepEqual(await storage.listPinnedSources(), []);
});

test("源 ID 可识别但元数据损坏时仍可选择清理", { timeout: 10000 }, async () => {
  const fake = fakeFactory(), id = "d".repeat(32);
  fake.entries.set(`wasm-handover-source:${id}`, "old"); fake.entries.set(`wasm-handover-meta:${id}:bad`, "");
  const storage = new IndexedDbSaveStorage(fake.factory);
  assert.deepEqual(await storage.listPinnedSources(), [{ sourceId: id, logicalBytes: null, seed: null }]);
  await storage.deletePinnedSource(id); assert.equal(fake.entries.size, 0);
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
