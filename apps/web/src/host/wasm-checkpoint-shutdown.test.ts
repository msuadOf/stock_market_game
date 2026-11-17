import assert from "node:assert/strict";
import { test } from "node:test";
import { CheckpointCloseError } from "./wasm-checkpoint-client.ts";
import { closeCheckpointAndCleanup } from "./wasm-checkpoint-shutdown.ts";

test("已确认归档退出但未耐久仍执行 Rust 清理并保留失败", async () => {
  let cleaned = 0;
  const result = await closeCheckpointAndCleanup(() => Promise.reject(new CheckpointCloseError("confirmed", new Error("未耐久"))), () => ++cleaned);
  assert.equal(cleaned, 1);
  assert.equal(result.value, 1);
  assert.equal(result.archiveFailure?.kind, "nondurable");
});
test("未知归档退出不触碰同一 Memory 的 Rust 清理", async () => {
  let cleaned = false;
  await assert.rejects(closeCheckpointAndCleanup(() => Promise.reject(new CheckpointCloseError("unknown", new Error("未退出"))), () => { cleaned = true; }), /未退出/);
  assert.equal(cleaned, false);
});
test("归档主错和后续 Rust 清理失败同时保留", async () => {
  await assert.rejects(closeCheckpointAndCleanup(() => Promise.reject(new CheckpointCloseError("confirmed", new Error("未耐久"))), () => { throw new Error("析构失败"); }), (error: unknown) => error instanceof AggregateError && error.errors.length === 2 && String(error.cause).includes("未耐久"));
});
