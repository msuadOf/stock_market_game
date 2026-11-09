import { strict as assert } from "node:assert";
import { test } from "node:test";
import { IndexedDbSaveRepository } from "./indexeddb-save-repository.ts";

test("开库阻塞后允许重试，旧请求迟到成功时关闭旧连接", async () => {
  const requests: Array<{ onblocked: (() => void) | null; onerror: (() => void) | null; onsuccess: (() => void) | null; result?: unknown; error?: Error }> = [];
  const factory = {
    open() {
      const request = { onblocked: null, onerror: null, onsuccess: null };
      requests.push(request);
      return request;
    },
  } as unknown as IDBFactory;
  const repository = new IndexedDbSaveRepository(factory);
  const first = repository.list();
  const concurrent = repository.list();
  assert.equal(requests.length, 1);
  requests[0].onblocked!();
  await assert.rejects(first, /阻塞/);
  await assert.rejects(concurrent, /阻塞/);

  const retry = repository.list();
  assert.equal(requests.length, 2);
  let closed = false;
  requests[0].result = { close() { closed = true; } };
  requests[0].onsuccess!();
  assert.equal(closed, true);
  requests[1].error = new Error("第二次开库失败");
  requests[1].onerror!();
  await assert.rejects(retry, /第二次开库失败/);
});
