import assert from "node:assert/strict";
import { test } from "node:test";
import { chooseCheckpointWrite, WorkerCheckpointRepository } from "./worker-checkpoint-repository.ts";
import { QUICK_SAVE_KEY } from "./save-storage-keys.ts";

test("快速槽正文和递增的租约水位同一次事务写入", () => {
  const writes = chooseCheckpointWrite({ lease: "active", seq: "4" }, "active", 5n, "gzip:payload");
  assert.deepEqual(writes, [
    [QUICK_SAVE_KEY + ":writer", { lease: "active", seq: "5" }],
    [QUICK_SAVE_KEY, "gzip:payload"],
  ]);
});

test("恢复换代后旧写者和重复序列均不能覆盖快速槽", () => {
  const current = { lease: "new", seq: "20" };
  assert.equal(chooseCheckpointWrite(current, "old", 30n, "gzip:old"), null);
  assert.equal(chooseCheckpointWrite(current, "new", 20n, "gzip:duplicate"), null);
  assert.throws(() => chooseCheckpointWrite({ lease: "new", seq: "nan" }, "new", 21n, "gzip:broken"));
});

test("归档副实例在首次日结前不新建数据库", async () => {
  const factory = {
    databases: async () => [{ name: "无关数据库", version: 1 }],
    open: () => { throw new Error("不存在的快速槽不得在启动时打开"); },
  } as unknown as IDBFactory;
  const repository = new WorkerCheckpointRepository(factory, "initial");
  await repository.activate();
  await repository.close();
});

test("首份日终档在同一事务认领空槽，但不能接管另一副实例已经认领的槽", () => {
  assert.equal(chooseCheckpointWrite(undefined, "initial", 5n, "gzip:payload"), null);
  assert.deepEqual(chooseCheckpointWrite(undefined, "initial", 5n, "gzip:payload", true), [
    [QUICK_SAVE_KEY + ":writer", { lease: "initial", seq: "5" }],
    [QUICK_SAVE_KEY, "gzip:payload"],
  ]);
  assert.equal(chooseCheckpointWrite({ lease: "other", seq: "-1" }, "initial", 5n, "gzip:payload", true), null);
});
