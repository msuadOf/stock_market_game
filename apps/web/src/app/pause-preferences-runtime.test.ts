import assert from "node:assert/strict";
import test from "node:test";
import { createPausePreferencesLifecycle } from "./usePausePreferences.ts";

function fixture() {
  const writes: string[] = [], errors: string[] = [], applied: unknown[] = [];
  let reads = 0, ready = 0;
  const storage = { getItem: () => { reads++; return '{"pause_after_close":true,"pause_before_open":false}'; }, setItem: (_key: string, value: string) => { writes.push(value); } };
  const runtime = createPausePreferencesLifecycle({ apply: (value) => { applied.push(value); }, onReady: () => { ready++; }, onError: (error) => errors.push(error) });
  return { runtime, storage, writes, errors, applied, reads: () => reads, ready: () => ready };
}
test("暂停偏好只首次加载一次；缺少 storage 不伪造 ready", () => {
  const f = fixture(); f.runtime.loadOnce(null); assert.equal(f.ready(), 0);
  f.runtime.loadOnce(f.storage); f.runtime.loadOnce(f.storage);
  assert.equal(f.reads(), 1); assert.equal(f.ready(), 1); assert.deepEqual(f.applied, [{ pause_after_close: true, pause_before_open: false }]);
});
test("暂停偏好加载失败保持启动关闭，错误显式展示", () => {
  const f = fixture(); f.runtime.loadOnce({ ...f.storage, getItem: () => { throw new Error("读取被禁止"); } });
  assert.equal(f.ready(), 0); assert.match(f.errors[0], /读取被禁止/);
});
test("host 未就绪与 E2E skip 均不写偏好，host/storage 失败独立报告", async () => {
  const f = fixture(), preferences = { pause_after_close: false, pause_before_open: true };
  f.runtime.loadOnce(f.storage); await f.runtime.synchronize(null, preferences, f.storage, false); assert.equal(f.writes.length, 0);
  let calls = 0;
  const host = { setPausePreferences: async () => { calls++; throw new Error("宿主失败"); } };
  await f.runtime.synchronize(host, preferences, f.storage, true); assert.equal(calls, 0);
  await f.runtime.synchronize(host, preferences, f.storage, false);
  assert.equal(calls, 1); assert.equal(f.writes.length, 0); assert.equal(f.applied.length, 1);
  await f.runtime.synchronize({ setPausePreferences: async () => {} }, preferences, { ...f.storage, setItem: () => { throw new Error("写入被禁止"); } }, false);
  assert.equal(f.errors.length, 2); assert.ok(f.errors.some((error) => /写入被禁止/.test(error))); assert.ok(f.errors.some((error) => /宿主失败/.test(error)));
});
test("sessionStorage 属性读取失败同样显式展示；host 同步仍先执行", async () => {
  const f = fixture();
  f.runtime.loadOnce(() => { throw new Error("storage getter 失败"); });
  assert.equal(f.ready(), 0); assert.match(f.errors[0], /getter 失败/);
  f.runtime.loadOnce(f.storage);
  let calls = 0;
  await f.runtime.synchronize({ setPausePreferences: async () => { calls++; } }, { pause_after_close: true, pause_before_open: true },
    () => { throw new Error("storage getter 写入失败"); }, false);
  assert.equal(calls, 1); assert.match(f.errors[1], /getter 写入失败/);
});

test("G40：暂停偏好等actor确认再发布UI/storage，旧宿主确认不能落盘", { timeout: 10000 }, async () => {
  const f = fixture(); f.runtime.loadOnce(f.storage);
  let confirm!: () => void;
  const host = { setPausePreferences: () => new Promise<void>((resolve) => { confirm = resolve; }) };
  const preferences = { pause_after_close: false, pause_before_open: true };
  const changed = f.runtime.synchronize(host, preferences, f.storage, false);
  await Promise.resolve();
  assert.equal(f.writes.length, 0); assert.equal(f.applied.length, 1);
  confirm(); await changed;
  assert.deepEqual(f.applied.at(-1), preferences); assert.equal(f.writes.length, 1);
  const stale = f.runtime.synchronize(host, { ...preferences, pause_after_close: true }, f.storage, false, () => false);
  await Promise.resolve(); confirm(); await stale;
  assert.equal(f.writes.length, 1); assert.equal(f.applied.length, 2);
});

test("G40：旧宿主晚到拒绝不污染新局，拒绝后队列可继续确认", { timeout: 10000 }, async () => {
  const f = fixture(); f.runtime.loadOnce(f.storage);
  let reject!: (error: Error) => void;
  let current = true;
  const preferences = { pause_after_close: false, pause_before_open: true };
  const changed = f.runtime.synchronize({ setPausePreferences: () => new Promise<void>((_resolve, fail) => { reject = fail; }) }, preferences, f.storage, false, () => current);
  await Promise.resolve(); current = false; reject(new Error("旧会话 IPC 失败")); await changed;
  assert.equal(f.errors.length, 0); assert.equal(f.writes.length, 0);
  await f.runtime.synchronize({ setPausePreferences: async () => { throw new Error("当前失败"); } }, preferences, f.storage, false);
  assert.equal(f.errors.length, 1); assert.match(f.errors[0]!, /当前失败/);
  await f.runtime.synchronize({ setPausePreferences: async () => {} }, preferences, f.storage, false);
  assert.deepEqual(f.applied.at(-1), preferences); assert.equal(f.writes.length, 1);
});
