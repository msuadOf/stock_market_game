import assert from "node:assert/strict";
import test from "node:test";
import { readDecodedText } from "./archive-decoded-reader.ts";

test("正常 EOF 释放 reader 但不取消", { timeout: 10000 }, async () => {
  const calls: string[] = []; let count = 0;
  const reader = { async read() { return count++ === 0 ? { done: false as const, value: "甲" } : { done: true as const, value: undefined }; }, async cancel() { calls.push("cancel"); }, releaseLock() { calls.push("release"); } };
  const output: string[] = []; for await (const chunk of readDecodedText(reader)) output.push(chunk);
  assert.deepEqual(output, ["甲"]); assert.deepEqual(calls, ["release"]);
});

test("提前结束等待 cancel 完成后才释放", { timeout: 10000 }, async () => {
  const calls: string[] = []; let releaseCancel!: () => void; let cancelStarted!: () => void;
  const gate = new Promise<void>(resolve => { releaseCancel = resolve; });
  const started = new Promise<void>(resolve => { cancelStarted = resolve; });
  const reader = { async read() { return { done: false as const, value: "甲" }; }, async cancel() { calls.push("cancel"); cancelStarted(); await gate; calls.push("cancelled"); }, releaseLock() { calls.push("release"); } };
  const iterator = readDecodedText(reader); await iterator.next(); const closing = iterator.return(undefined);
  await started; assert.deepEqual(calls, ["cancel"]); releaseCancel(); await closing;
  assert.deepEqual(calls, ["cancel", "cancelled", "release"]);
});

test("pending read 取消可唤醒并保持原因", { timeout: 10000 }, async () => {
  const calls: string[] = []; const controller = new AbortController(); const reason = new Error("取消");
  const reader = { read: () => new Promise<ReadableStreamReadResult<string>>(() => {}), async cancel() { calls.push("cancel"); }, releaseLock() { calls.push("release"); } };
  const pending = readDecodedText(reader, controller.signal).next(); controller.abort(reason);
  await assert.rejects(pending, error => error === reason); assert.deepEqual(calls, ["cancel", "release"]);
});

test("读取失败及取消失败同时保留", { timeout: 10000 }, async () => {
  const primary = new Error("读取失败"), cleanup = new Error("取消失败"); const calls: string[] = [];
  const reader = { async read(): Promise<ReadableStreamReadResult<string>> { throw primary; }, async cancel() { throw cleanup; }, releaseLock() { calls.push("release"); } };
  await assert.rejects(readDecodedText(reader).next(), error => error instanceof AggregateError && error.errors[0] === primary && error.errors[1] === cleanup);
  assert.deepEqual(calls, ["release"]);
});

test("EOF 后 releaseLock 单独失败仍明确标为清理失败", { timeout: 10000 }, async () => {
  const cleanup = new Error("releaseLock 失败");
  const reader = { async read() { return { done: true as const, value: undefined }; }, async cancel() {}, releaseLock() { throw cleanup; } };
  await assert.rejects(readDecodedText(reader).next(), error => error instanceof AggregateError && error.errors.length === 1 && error.errors[0] === cleanup);
});
