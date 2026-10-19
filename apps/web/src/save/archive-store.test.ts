import assert from "node:assert/strict";
import test from "node:test";
import { createTransportArchiveStore, parseArchiveMetadata } from "./archive-store.ts";

const entry = { slot_id: "current", name: "日终档", civil_date: "2030-01-11", tick: 240 };

test("ArchiveStore 元数据严格拒绝旧字段、坏单位与重复槽位", { timeout: 10000 }, async () => {
  assert.deepEqual(parseArchiveMetadata(entry), entry);
  assert.throws(() => parseArchiveMetadata({ ...entry, schema_version: 1 }), /元数据/);
  assert.throws(() => parseArchiveMetadata({ ...entry, tick: "240" }), /元数据/);
  assert.throws(() => parseArchiveMetadata({ ...entry, name: " " }), /元数据/);
  assert.throws(() => parseArchiveMetadata({ ...entry, civil_date: "2030-02-30" }), /civil_date/);
  const repository = createTransportArchiveStore(async () => [entry, entry]);
  await assert.rejects(repository.list(), /重复槽位/);
});

test("ArchiveStore 元数据管理不请求加载，错误不会降级为空列表", { timeout: 10000 }, async () => {
  const calls: unknown[][] = [];
  const repository = createTransportArchiveStore(async (operation, slotId, name) => {
    calls.push([operation, slotId, name]);
    if (operation === "list") return [entry];
    if (operation === "copy") return { ...entry, slot_id: "copy", name };
    if (operation === "load") throw new Error("元数据操作不能调用load");
    return null;
  });
  assert.deepEqual(await repository.list(), [entry]);
  await repository.rename("current", "新名");
  assert.deepEqual(await repository.copy("current", "复制"), { ...entry, slot_id: "copy", name: "复制" });
  await repository.delete("copy");
  assert.deepEqual(calls, [["list", undefined, undefined], ["rename", "current", "新名"], ["copy", "current", "复制"], ["delete", "copy", undefined]]);
  await assert.rejects(createTransportArchiveStore(async () => { throw new Error("权限不足"); }).list(), /权限不足/);
  await assert.rejects(createTransportArchiveStore(async () => ({})).list(), /列表结构/);
  await assert.rejects(createTransportArchiveStore(async () => true).rename("current", "名称"), /响应结构/);
});

test("ArchiveStore 选择元数据须确认提交，旧客户端响应不能冒称当前选择成功", { timeout: 10000 }, async () => {
  const calls: unknown[][] = [];
  const repository = createTransportArchiveStore(async (operation, slotId) => { calls.push([operation, slotId]); return true; });
  assert.equal(await repository.select("chosen"), true);
  assert.equal(await repository.select("retired", () => false), false);
  assert.deepEqual(calls, [["select", "chosen"]]);
  await assert.rejects(createTransportArchiveStore(async () => null).select("chosen"), /启动槽选择响应结构/);
  assert.equal(await createTransportArchiveStore(async () => null).load(), null);
  let release!: (value: boolean) => void;
  let current = true;
  const delayed = createTransportArchiveStore(async () => new Promise<boolean>((resolve) => { release = resolve; }));
  const selecting = delayed.select("chosen", () => current);
  current = false;
  release(true);
  assert.equal(await selecting, false);
});
