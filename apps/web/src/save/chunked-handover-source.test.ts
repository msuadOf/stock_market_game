import assert from "node:assert/strict";
import test from "node:test";
import { captureGzipArchiveSource } from "./archive-source.ts";
import { createValidatedArchiveLease } from "./validated-archive-lease.ts";
import { gzipSaveCodec } from "./save-repository.ts";
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts";
import { createChunkedHandoverSources, inspectPinnedArchiveSource, parseChunkedSourceDescriptor } from "./chunked-handover-source.ts";

test("固定已验证源并在快速槽变化后恢复同一存档", { timeout: 10000 }, async () => {
  const raw = `gzip:${await gzipSaveCodec.encode(JSON.stringify(withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")))}`;
  const lease = await createValidatedArchiveLease(captureGzipArchiveSource(raw));
  const data = new Map<string, string>();
  const adapter = createChunkedHandoverSources({
    async getItem(key) { return data.get(key) ?? null; },
    async addItem(key, value, current) { if (!current()) return false; data.set(key, value); return true; }, async removeItem(key) { data.delete(key); },
  }, () => "0123456789abcdef0123456789abcdef");
  const signal = new AbortController().signal;
  const pinned = await adapter.pin(lease, signal);
  const other = createChunkedHandoverSources({ async getItem() { return null }, async addItem() { return true }, async removeItem() {} });
  assert.throws(() => other.inspect(pinned, lease), /不属于/);
  assert.equal(await adapter.pin(lease, signal), pinned);
  const descriptor = inspectPinnedArchiveSource(pinned, lease);
  assert.deepEqual(parseChunkedSourceDescriptor(structuredClone(descriptor)), descriptor);
  assert.throws(() => parseChunkedSourceDescriptor({ ...descriptor, extra: true }), /字段/);
  assert.throws(() => parseChunkedSourceDescriptor({ ...descriptor, contentDigest: { ...descriptor.contentDigest, blocks: "999" } }), /摘要/);
  assert.equal(data.get(`wasm-handover-source:${descriptor.sourceId}`), raw);
  data.delete(`wasm-handover-source:${descriptor.sourceId}`);
  await assert.rejects(adapter.withPinnedPublication(pinned, lease, signal, () => { throw new Error("不应发布"); }), /不存在/);
  data.set(`wasm-handover-source:${descriptor.sourceId}`, raw);
  data.set("quick-save", "另一份档");
  const restored = await adapter.resolve(descriptor, signal);
  assert.notEqual(restored, lease);
  assert.throws(() => inspectPinnedArchiveSource(structuredClone(pinned), lease), /凭证/);
  const altered = { ...withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02"), seed: descriptor.seed === "1" ? "2" : "1" };
  data.set(`wasm-handover-source:${descriptor.sourceId}`, `gzip:${await gzipSaveCodec.encode(JSON.stringify(altered))}`);
  await assert.rejects(adapter.resolve(descriptor, signal), /摘要不一致/);
  data.set(`wasm-handover-source:${descriptor.sourceId}`, "gzip:损坏");
  await assert.rejects(adapter.resolve(descriptor, signal));
  data.delete(`wasm-handover-source:${descriptor.sourceId}`);
  await assert.rejects(adapter.pin(lease, signal), /已删除|已改变/);
  assert.notEqual(await adapter.pin(lease, signal), pinned);
});

test("取消后等待真实写入结束，旧操作不得签发凭证", { timeout: 10000 }, async () => {
  const raw = `gzip:${await gzipSaveCodec.encode(JSON.stringify(withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")))}`;
  const lease = await createValidatedArchiveLease(captureGzipArchiveSource(raw));
  const controller = new AbortController(); let finish: (() => void) | undefined, settled = false;
  const adapter = createChunkedHandoverSources({ async getItem() { return null }, addItem(_key, _value, current) {
    return new Promise<boolean>(resolve => { finish = () => resolve(current()); });
  }, async removeItem() {} }, () => "ef".repeat(16));
  const attempt = adapter.pin(lease, controller.signal).then(() => { settled = true; }, () => { settled = true; });
  controller.abort(new Error("选择已更换")); await Promise.resolve();
  assert.equal(settled, false); assert.ok(finish); finish(); await attempt; assert.equal(settled, true);
});

test("提交后取消须删除未发布源", { timeout: 10000 }, async () => {
  const raw = `gzip:${await gzipSaveCodec.encode(JSON.stringify(withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")))}`;
  const lease = await createValidatedArchiveLease(captureGzipArchiveSource(raw));
  const controller = new AbortController(); let finish: (() => void) | undefined, deleted = false;
  const adapter = createChunkedHandoverSources({ async getItem() { return null }, addItem() { return new Promise(resolve => { finish = () => resolve(true); }); }, async removeItem() { deleted = true; } }, () => "ab".repeat(16));
  const attempt = adapter.pin(lease, controller.signal);
  controller.abort(new Error("更换选择")); assert.ok(finish); finish();
  await assert.rejects(attempt, /更换选择/); assert.equal(deleted, true);
});

test("发布与删除使用同一锁，发布前若源已删除则拒绝冻结", { timeout: 10000 }, async () => {
  const raw = `gzip:${await gzipSaveCodec.encode(JSON.stringify(withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")))}`;
  const lease = await createValidatedArchiveLease(captureGzipArchiveSource(raw));
  const data = new Map<string, string>(); let tail = Promise.resolve(); const order: string[] = [];
  const lock = <T>(_id: string, _signal: AbortSignal, action: () => Promise<T>) => {
    const result = tail.then(action); tail = result.then(() => {}, () => {}); return result;
  };
  let finish!: () => void; const gate = new Promise<void>(resolve => { finish = resolve; });
  const adapter = createChunkedHandoverSources({ async getItem(key) { return data.get(key) ?? null }, async addItem(key, value) { data.set(key, value); return true }, async removeItem(key) { data.delete(key) } }, () => "ab".repeat(16), lock);
  const signal = new AbortController().signal, pin = await adapter.pin(lease, signal), descriptor = adapter.inspect(pin, lease);
  const first = lock(descriptor.sourceId, signal, async () => { order.push("持锁"); await gate; data.delete(`wasm-handover-source:${descriptor.sourceId}`); order.push("删除"); });
  const second = adapter.withPinnedPublication(pin, lease, signal, () => { order.push("发布"); });
  await Promise.resolve(); finish(); await first;
  await assert.rejects(second, /不存在/); assert.deepEqual(order, ["持锁", "删除"]);
});
