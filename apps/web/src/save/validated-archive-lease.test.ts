import assert from "node:assert/strict";
import test from "node:test";
import { captureGzipArchiveSource } from "./archive-source.ts";
import { gzipSaveCodec } from "./save-repository.ts";
import { currentSaveFixture, withSettledDay } from "./current-save-fixture.ts";
import { createValidatedArchiveLease, inspectValidatedArchiveLease } from "./validated-archive-lease.ts";

async function capture(value: unknown) {
  const text = JSON.stringify(value);
  return { text, source: captureGzipArchiveSource(`gzip:${await gzipSaveCodec.encode(text)}`) };
}

test("仅完整 schema 与日终成功后签发同源不可伪造租约", { timeout: 10000 }, async () => {
  const { text, source } = await capture(withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02"));
  const lease = await createValidatedArchiveLease(source);
  const details = inspectValidatedArchiveLease(lease);
  assert.equal(details.source, source); assert.equal(details.utf8Bytes, Buffer.byteLength(text));
  assert.equal(details.seed, "42"); assert.ok(Object.isFrozen(details.setup)); assert.ok(Object.isFrozen(details.setup.stocks));
  assert.equal(details.productionAdmission, false); assert.ok(Object.isFrozen(lease));
  assert.equal(details.contentDigest.algorithm, "sha256-chain-v1");
  assert.equal(details.contentDigest.byteLength, BigInt(Buffer.byteLength(text))); assert.ok(Object.isFrozen(details.contentDigest));
  assert.ok(Object.isFrozen(details));
  assert.throws(() => inspectValidatedArchiveLease(structuredClone(lease)), /租约身份/);
  assert.throws(() => inspectValidatedArchiveLease({ ...lease }), /租约身份/);
});

test("错误的 schema、未日结、伪造源和取消均不能签发", { timeout: 10000 }, async () => {
  const invalid = await capture({}); const unsettled = await capture(currentSaveFixture());
  await assert.rejects(createValidatedArchiveLease(invalid.source));
  await assert.rejects(createValidatedArchiveLease(unsettled.source));
  await assert.rejects(createValidatedArchiveLease(structuredClone(invalid.source)), /捕获的身份/);
  const controller = new AbortController(); const reason = new Error("用户取消"); controller.abort(reason);
  await assert.rejects(createValidatedArchiveLease(invalid.source, controller.signal), error => error === reason);
  const active = new AbortController(); const cancelled = new Error("接收中取消");
  const pending = createValidatedArchiveLease(invalid.source, active.signal); active.abort(cancelled);
  await assert.rejects(pending, error => error === cancelled);
});

test("外部篡改 codec 不得替换租约验证所读取的数据", { timeout: 10000 }, async () => {
  const { source } = await capture({});
  assert.throws(() => {
    gzipSaveCodec.decodeChunks = async function* () { yield JSON.stringify(withSettledDay(currentSaveFixture(), "2030-01-01", "2030-01-02")); };
  }, TypeError);
  await assert.rejects(createValidatedArchiveLease(source));
});

test("真实 gzip 解码提前退出可完成取消", { timeout: 10000 }, async () => {
  const compressed = await gzipSaveCodec.encode("中文".repeat(100_000));
  const decode = gzipSaveCodec.decodeChunks;
  assert.ok(decode);
  const reader = decode(compressed)[Symbol.asyncIterator]();
  const first = await reader.next(); assert.equal(first.done, false);
  await reader.return?.();
});
