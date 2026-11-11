import assert from "node:assert/strict";
import { test } from "node:test";
import { buildErrorFeedback, copyErrorFeedback } from "./error-details.ts";

test("反馈原样保留原始错误、任意 context、非枚举 stack 与 AggregateError 原因", async () => {
  const inner = new TypeError("未知证券 ID=77 /Users/test/save.json");
  inner.stack = "TypeError: 未知证券 ID=77\n    at engine.commit (/Users/test/engine.ts:12:3)";
  const cause = new AggregateError([inner, { raw: "原始异常", amount: 123456789n }], "批次失败", { cause: inner });
  const failure = Object.assign(new Error("请求失败 https://localhost:4173/game?token=example", { cause }), {
    code: "HOST_FATAL", where: "worker.response", context: {
      actual: "account-77", operation: "restoreArchive", rawSnippet: "{invalid json}",
      reason: "ARCHIVE_SCHEMA_MISMATCH", entries: { "custom-field": "完整上下文" },
    },
  });
  failure.stack = "Error: 请求失败\n    at response (/Users/test/host.ts:99:8)";
  const text = buildErrorFeedback(failure);
  for (const expected of [failure.message, failure.stack, inner.stack, "AggregateError", "123456789", "restoreArchive", "ARCHIVE_SCHEMA_MISMATCH", "{invalid json}", "custom-field", "完整上下文"]) {
    assert.ok(text.includes(expected.replaceAll("\n", "\\n")) || text.includes(expected), expected);
  }
  assert.doesNotMatch(text, /已脱敏/);
  let copied = "";
  await copyErrorFeedback(failure, () => {}, { writeText: async (value) => { copied = value; } });
  assert.equal(copied, text);
  assert.equal(failure.context.actual, "account-77");
});

test("原始诊断保留深层详情，循环与访问器显式标记且不执行 getter", () => {
  let reads = 0;
  const context: Record<string, unknown> = {};
  Object.defineProperty(context, "unreadable", { get() { reads++; throw new Error("getter 不应执行"); } });
  let nested = context;
  for (let index = 0; index < 20; index++) { const next = {}; nested.next = next; nested = next; }
  nested.reason = "深层真实原因";
  nested.original = context;
  const text = buildErrorFeedback({ code: "HOST_FATAL", where: "host", message: "失败", context });
  assert.match(text, /深层真实原因/);
  assert.match(text, /循环引用/);
  assert.match(text, /访问器字段未执行/);
  assert.equal(reads, 0);
});

test("结构化克隆可传输的 Map、Set、Date 与 typed array 诊断不会变成空对象", () => {
  const text = buildErrorFeedback({ code: "HOST_FATAL", where: "host", message: "失败", context: {
    records: new Map([["原始索引键", { reason: "找不到候选" }]]),
    owners: new Set(["engine-worker-77"]),
    date: new Date("2030-01-02T03:04:05Z"),
    bytes: new Uint8Array([31, 42]),
  } });
  for (const expected of ["原始索引键", "找不到候选", "engine-worker-77", "2030-01-02T03:04:05.000Z", "31", "42"]) assert.ok(text.includes(expected), expected);
});
