import assert from "node:assert/strict";
import test from "node:test";
import { clientDayEndTargets, writeDayEndTargets } from "./day-end-targets.ts";

test("remote 未授权文件时没有客户端日终目标，不访问浏览器快速槽", { timeout: 10000 }, () => {
  for (const kind of ["remote"] as const) {
    let browserAccesses = 0;
    const targets = clientDayEndTargets(kind, () => { browserAccesses += 1; throw new Error(`${kind} 不应访问 IndexedDB`); }, null);
    assert.deepEqual(targets, []);
    assert.equal(browserAccesses, 0);
  }
});

test("remote 显式授权文件时只更新该文件，不访问浏览器快速槽", { timeout: 10000 }, async () => {
  for (const kind of ["remote"] as const) {
    let fileWrites = 0;
    const authorizedFile = { label: "授权文件", write: async () => { fileWrites += 1; } };
    const targets = clientDayEndTargets(kind, () => { throw new Error(`${kind} 不应访问 IndexedDB`); }, authorizedFile);
    assert.deepEqual(targets, [authorizedFile]);
    assert.equal(await writeDayEndTargets({}, () => true, targets), true);
    assert.equal(fileWrites, 1);
  }
});

test("wasm 与当前 tauri 保留快速槽及显式授权文件日终目标", { timeout: 10000 }, async () => {
  for (const kind of ["wasm", "tauri"] as const) {
    const writes: string[] = [];
    const quickSlot = { label: "浏览器快速槽", write: async () => { writes.push("quick"); } };
    const authorizedFile = { label: "授权文件", write: async () => { writes.push("file"); } };
    assert.deepEqual(clientDayEndTargets(kind, () => quickSlot, null), [quickSlot]);
    const targets = clientDayEndTargets(kind, () => quickSlot, authorizedFile);
    assert.equal(await writeDayEndTargets({}, () => true, targets), true);
    assert.deepEqual(writes, ["quick", "file"]);
  }
});

test("an exhausted quick slot does not prevent updating the authorized day-end file", async () => {
  let fileWrites = 0;
  await assert.rejects(writeDayEndTargets({ day: 1 }, () => true, [
    { label: "快速槽", write: async () => { throw new Error("quota exhausted"); } },
    { label: "授权文件", write: async () => { fileWrites += 1; } },
  ]), /快速槽.*quota exhausted.*授权文件.*已更新/);
  assert.equal(fileWrites, 1);
});

test("day-end target failures preserve both causes and report each successful target honestly", async () => {
  await assert.rejects(writeDayEndTargets({}, () => true, [
    { label: "快速槽", write: async () => { throw new Error("quota exhausted"); } },
    { label: "授权文件", write: async () => { throw new Error("disk full"); } },
  ]), (error: unknown) => error instanceof AggregateError && error.errors.length === 2
    && /quota exhausted/.test(String(error.errors[0])) && /disk full/.test(String(error.errors[1])));
  await assert.rejects(writeDayEndTargets({}, () => true, []), /没有日终输出目标/);
});

test("a stale quick-slot skip is never reported as an actual update", async () => {
  await assert.rejects(writeDayEndTargets({}, () => false, [
    { label: "快速槽", write: async () => false },
    { label: "授权文件", write: async () => { throw new Error("old generation"); } },
  ]), (error: unknown) => error instanceof AggregateError && /快速槽已取消/.test(error.message)
    && !/快速槽已更新/.test(error.message));
  assert.equal(await writeDayEndTargets({}, () => false, [{ label: "快速槽", write: async () => false }]), false);
});
