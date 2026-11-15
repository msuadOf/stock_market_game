import assert from "node:assert/strict";
import test from "node:test";
import { parseRustSessionShutdown, parseWasmSessionShutdown } from "./wasm-shutdown-report.ts";

function completion() {
  return { owned_workers: 2, joined_workers: 2,
    worker_results: [{ worker: "group-0", result: { Ok: undefined } }, { worker: "desk-0", result: { Ok: undefined } }], group_finals: 1, desk_finals: 1 };
}
test("Rust 真实 serde Option 与 Result 形状保留两次 join 和 Final", { timeout: 10000 }, () => {
  const parsed = parseRustSessionShutdown({ handle: 7, result: { runtime: completion() } }, "success", 7);
  assert.equal(parsed.kind, "success", "SHUTDOWN_REAL_REPORT_REQUIRED");
  if (parsed.kind !== "success") throw new Error("预期成功分支");
  assert.deepEqual(parsed.runtime, { ownedWorkers: 2, joinedWorkers: 2, workers: [{ worker: "group-0", result: { kind: "ok" } }, { worker: "desk-0", result: { kind: "ok" } }], groupFinals: 1, deskFinals: 1 });
  assert.deepEqual(parseRustSessionShutdown({ handle: 7, result: { runtime: undefined } }, "success", 7), { kind: "success", handle: 7, runtime: null });
});
test("失败主错、两次错和已 join 事实均保留，不 String 降格", { timeout: 10000 }, () => {
  const prior = completion(); prior.group_finals = 0; prior.desk_finals = 0;
  prior.worker_results[0].result = { Err: "first worker failed" } as never;
  const parsed = parseRustSessionShutdown({ handle: 7, result: {
    primary: { stage: "Drain", worker: "group-0", detail: "primary" },
    secondary: [{ stage: "join", worker: "group-0", detail: "secondary-1" }, { stage: "join", worker: "desk-0", detail: "secondary-2" }], completion: prior,
  } }, "failure", 7);
  assert.equal(parsed.kind, "failure", "SHUTDOWN_ALL_ERRORS_REQUIRED");
  if (parsed.kind !== "failure") throw new Error("预期失败分支");
  assert.deepEqual(parsed.secondary.map((issue) => issue.detail), ["secondary-1", "secondary-2"]);
  assert.equal(parsed.completion?.joinedWorkers, 2);
  assert.deepEqual(parsed.completion?.workers[0].result, { kind: "error", message: "first worker failed" });
});
test("缺结果、重复身份、计数不符和错误 handle 均不得成功", { timeout: 10000 }, () => {
  const noResult = completion(); delete (noResult.worker_results[0] as { result?: unknown }).result;
  const duplicate = completion(); duplicate.worker_results[1].worker = "group-0";
  const wrongCount = { ...completion(), joined_workers: 1 };
  const wrongFinal = { ...completion(), group_finals: 0 };
  for (const runtime of [noResult, duplicate, wrongCount, wrongFinal]) {
    assert.throws(() => parseRustSessionShutdown({ handle: 7, result: { runtime } }, "success", 7));
  }
  assert.throws(() => parseRustSessionShutdown({ handle: 8, result: { runtime: completion() } }, "success", 7));
});

test("join 分类跨 Rust 与 Worker 边界，主错保留且不接收 panic payload", { timeout: 10000 }, () => {
  const wire = { handle: 7, result: {
    primary: { stage: "Drain", worker: "group-0", detail: "原始 Drain 错误" },
    secondary: [
      { stage: "worker_join", worker: "group-0", detail: "死亡", join_failure: { kind: "worker_hard_trap", code: 0xD5 } },
      { stage: "worker_join", worker: "desk-0", detail: "join panic", join_failure: { kind: "native_panic" } },
      { stage: "worker_join", worker: "desk-1", detail: "传输失败", join_failure: { kind: "transport" } },
    ],
    completion: undefined,
  } };
  const report = parseRustSessionShutdown(wire, "failure", 7);
  if (report.kind !== "failure") throw new Error("预期失败分支");
  assert.equal(report.primary.detail, "原始 Drain 错误");
  assert.deepEqual(report.secondary.map((issue) => issue.joinFailure), [
    { kind: "worker_hard_trap", code: 0xD5 }, { kind: "native_panic" }, { kind: "transport" },
  ]);
  assert.deepEqual(parseWasmSessionShutdown(report), report);
  for (const malformed of [
    { kind: "worker_hard_trap", code: -1 },
    { kind: "worker_panic", code: 0x1_0000_0000 },
    { kind: "native_panic", payload: "不透明的 Rust 数据" },
  ]) {
    const secondary = [{ stage: "worker_join", worker: "group-0", detail: "死亡", join_failure: malformed }];
    assert.throws(() => parseRustSessionShutdown({ ...wire, result: { ...wire.result, secondary } }, "failure", 7));
  }
});
