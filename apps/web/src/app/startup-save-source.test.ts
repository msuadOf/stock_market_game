import assert from "node:assert/strict";
import test from "node:test";
import { InitialSaveSource } from "../save/session-replacement.ts";

test("同一次 App 启动取消加载或宿主失败再挂载时复用首次快速槽读取", async () => {
  const source = new InitialSaveSource<{ seed: string }>();
  let reads = 0;
  let resolveRead!: (value: { seed: string }) => void;
  const archive = { seed: "42" };
  const firstShell = source.read(() => {
    reads += 1;
    return new Promise((resolve) => { resolveRead = resolve; });
  });
  const secondShell = source.read(async () => { reads += 1; return { seed: "wrong" }; });
  assert.equal(firstShell, secondShell);
  await Promise.resolve();
  resolveRead(archive);
  assert.equal(await secondShell, archive);
  assert.equal(await source.read(async () => { reads += 1; return null; }), archive);
  assert.equal(reads, 1);
  source.complete();
  assert.equal(await source.read(async () => { reads += 1; return archive; }), null);
  assert.equal(reads, 1);
});

test("坏快速槽重选仍显式失败，只有明确新局或选择另一日终档才覆盖它", async () => {
  const source = new InitialSaveSource<string>();
  const cause = new Error("日终快速槽损坏");
  let reads = 0;
  const load = async () => { reads += 1; throw cause; };
  await assert.rejects(source.read(load), (error) => error === cause);
  await assert.rejects(source.read(load), (error) => error === cause);
  assert.equal(reads, 1);
  source.select("明确选择的其他日终档");
  assert.equal(await source.read(load), "明确选择的其他日终档");
  source.reset();
  assert.equal(await source.read(load), null);
  assert.equal(reads, 1);
});
