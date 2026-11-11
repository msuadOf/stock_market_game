import assert from "node:assert/strict";
import { registerHooks } from "node:module";
import test from "node:test";
import { captureGzipArchiveSource } from "./archive-source.ts";
import { gzipSaveCodec } from "./save-repository.ts";
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts";
import { createValidatedArchiveLease } from "./validated-archive-lease.ts";
import { replayValidatedArchive } from "./validated-archive-replay.ts";

async function fixture(subject = "😀") {
  const archive = withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02");
  const text = JSON.stringify(archive).replace('"local-owner"', JSON.stringify(subject));
  const source = captureGzipArchiveSource(`gzip:${await gzipSaveCodec.encode(text)}`);
  return { text, lease: await createValidatedArchiveLease(source) };
}

test("重放真实租约，所有分块有界且只有摘要核对后完成", { timeout: 10000 }, async () => {
  const { text, lease } = await fixture(); const chunks: Uint8Array[] = [];
  const replay = replayValidatedArchive(lease);
  for await (const chunk of replay) { assert.ok(chunk.byteLength <= 65536); chunks.push(chunk); }
  assert.equal(Buffer.concat(chunks).toString("utf8"), text);
  await assert.rejects(async () => { for await (const _chunk of replay) { /* 单次消费后不得重播 */ } }, /只能消费一次/);
  assert.throws(() => replayValidatedArchive(structuredClone(lease)), /租约身份/);
});

test("超过一个摘要块的 Unicode 内容仍按有界分块原样重放", { timeout: 10000 }, async () => {
  const { text, lease } = await fixture("😀".repeat(20000)); const chunks: Uint8Array[] = [];
  for await (const chunk of replayValidatedArchive(lease)) { assert.ok(chunk.byteLength <= 65536); chunks.push(chunk); }
  assert.ok(chunks.length > 1); assert.equal(Buffer.concat(chunks).toString("utf8"), text);
});

test("取消保留原因，不得成功完成", { timeout: 10000 }, async () => {
  const { lease } = await fixture(); const controller = new AbortController(); const reason = new Error("取消重放");
  const it = replayValidatedArchive(lease, controller.signal)[Symbol.asyncIterator]();
  assert.equal((await it.next()).done, false); controller.abort(reason);
  await assert.rejects(it.next(), error => error === reason);
});

test("消费者提前退出会取消实际 gzip reader", { timeout: 10000 }, async () => {
  const { lease } = await fixture("😀".repeat(20000));
  const original = ReadableStreamDefaultReader.prototype.cancel; let cancels = 0;
  ReadableStreamDefaultReader.prototype.cancel = function(reason?: unknown) { cancels++; return original.call(this, reason); };
  try {
    const it = replayValidatedArchive(lease)[Symbol.asyncIterator]();
    assert.equal((await it.next()).done, false); await it.return?.();
    assert.ok(cancels > 0);
  } finally { ReadableStreamDefaultReader.prototype.cancel = original; }
});

test("重放源字节漂移时不得得到完成，UTF-16 分片不破坏非 BMP 字符", { timeout: 10000 }, async () => {
  const { text, lease } = await fixture();
  const key = Symbol.for("archive-replay-test-override");
  Object.defineProperty(globalThis, key, { configurable: true, value: { text } });
  const hooks = registerHooks({
    resolve(specifier, context, next) {
      if (specifier === "./archive-source.ts" && context.parentURL?.includes("validated-archive-replay.ts?fixture")) return { url: "virtual:replay-source", shortCircuit: true };
      return next(specifier, context);
    },
    load(url, context, next) {
      if (url === "virtual:replay-source") return { format: "module", shortCircuit: true, source: `export async function* readCapturedArchiveChunks(){ const text=globalThis[Symbol.for("archive-replay-test-override")].text; for(const c of text.split("")) yield c; }` };
      return next(url, context);
    },
  });
  try {
    const moduleUrl: string = "./validated-archive-replay.ts?fixture";
    const { replayValidatedArchive: replay } = await import(moduleUrl);
    const bytes: Uint8Array[] = []; for await (const chunk of replay(lease)) bytes.push(chunk);
    assert.equal(Buffer.concat(bytes).toString("utf8"), text);
    const altered = text.replace('"seed":"42"', '"seed":"43"'); assert.notEqual(altered, text);
    (globalThis as unknown as Record<symbol, { text: string }>)[key]!.text = altered;
    await assert.rejects(async () => { for await (const _chunk of replay(lease)) { /* 排空以验证终态 */ } }, /摘要/);
  } finally { hooks.deregister(); Reflect.deleteProperty(globalThis, key); }
});
