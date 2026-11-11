import assert from "node:assert/strict";
import test from "node:test";
import { confirmChunkedSourceRelease } from "./wasm-chunked-source-release.ts";

const released = { kind: "not-allocated" as const };
test("release 必须等待源结束；普通失败但清理成功可以结账", { timeout: 10000 }, async () => {
  let resolve!: (value: { ok: false; error: unknown }) => void;
  const source = new Promise<{ ok: false; error: unknown }>(done => { resolve = done; });
  let finished = false; const result = confirmChunkedSourceRelease(released, source).finally(() => { finished = true; });
  await Promise.resolve(); assert.equal(finished, false);
  resolve({ ok: false, error: new Error("源读取失败但清理完成") });
  assert.equal(await result, released);
});
test("源清理超时或失败时不得发布 release", { timeout: 10000 }, async () => {
  const error = new AggregateError([new Error("清理超时")], "源状态未知");
  await assert.rejects(confirmChunkedSourceRelease(released, Promise.resolve({ ok: false, error })), value => value === error);
});
