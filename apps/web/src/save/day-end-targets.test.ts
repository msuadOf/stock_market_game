import assert from "node:assert/strict";
import test from "node:test";
import { writeDayEndTargets } from "./day-end-targets.ts";

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
