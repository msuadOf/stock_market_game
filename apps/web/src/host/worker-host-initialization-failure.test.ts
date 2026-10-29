import assert from "node:assert/strict";
import test from "node:test";
import { drain, fixture } from "./worker-host-w1.test-support.ts";
import type { HostFailure } from "./host-update.ts";

test("初始化prefix结构化failure拒绝时保留完整HostFailure并清理Worker", { timeout: 10000 }, async () => {
  const f = fixture();
  const originalCause = { phase: "restore-prefix", code: 1037 };
  const rawFailure = {
    type: "failure", code: "WASM_RESTORE_TOPOLOGY", where: "facade-engine-initialize.initial-archive",
    message: "日终存档前缀拓扑失败", cause: originalCause,
    context: { fault: 1037, operation: "restore-prefix", stockCount: 12 },
    recoverable: true, recoveryActions: ["保留存档", "反馈错误详情"],
  };
  try {
    const creating = f.launch();
    assert.equal(f.workers.length, 1);
    assert.equal(f.configured, 0, "prefix failure 必须发生在 engine substrate configured 之前");
    assert.equal(f.workers[0].sent.some((item) => item.type === "create" || item.type === "restoreInitial"), false, "prefix failure 前不得开始首建或初始恢复");
    f.workers[0].emit(rawFailure);
    await assert.rejects(creating, (error: unknown) => {
      assert.ok(error instanceof Error);
      assert.equal(error.message, "WASM_RESTORE_TOPOLOGY @ facade-engine-initialize.initial-archive: 日终存档前缀拓扑失败");
      assert.deepEqual((error as Error & { failure?: HostFailure }).failure, {
        code: rawFailure.code, where: rawFailure.where, message: rawFailure.message,
        cause: originalCause, context: rawFailure.context, recoverable: true, recoveryActions: ["保留存档", "反馈错误详情"],
      });
      assert.equal((error as Error & { cause?: unknown }).cause, (error as Error & { failure?: HostFailure }).failure);
      return true;
    });
    await drain();
    assert.equal(f.workers[0].sent.some((item) => item.type === "create" || item.type === "restoreInitial"), false, "prefix failure 后不得发生首建或初始恢复");
    assert.equal(f.readOutcome().status, "rejected", "初始化失败不得解析为host");
    assert.equal(f.workers[0].terminations, 1, "失败后必须终止engine Worker");
    assert.equal(f.workers[0].listenerCount(), 0, "失败后必须移除Worker监听器");
    assert.deepEqual(f.failures, [], "普通初始化失败不应伪装成spawn sink事件");
  } finally { await f.cleanup(); }
});

test("restore后段结构化failure发生在ready之后仍拒绝host并完整清理", { timeout: 10000 }, async () => {
  const f = fixture();
  const failure = {
    type: "failure", code: "WASM_RESTORE_SESSION", where: "wasm-session-slot.restore_json",
    message: "存档后段恢复失败", cause: { code: 2200, phase: "restore-runtime" },
    context: { operation: "restore", stage: "session-slot" }, recoverable: false,
    recoveryActions: ["保留错误详情"],
  };
  try {
    const creating = f.launch();
    await drain();
    assert.ok(f.workers[0].sent.some((item) => item.type === "create"), "配置ready后才进入首建阶段");
    f.workers[0].emit(failure);
    await assert.rejects(creating, (error: unknown) => {
      assert.ok(error instanceof Error);
      const initializationError = error as Error & { failure?: HostFailure; cause?: unknown };
      assert.equal(initializationError.failure?.code, failure.code);
      assert.equal(initializationError.failure?.where, failure.where);
      assert.equal(initializationError.failure?.message, failure.message);
      assert.deepEqual(initializationError.failure?.cause, failure.cause);
      assert.deepEqual(initializationError.failure?.context, failure.context);
      assert.equal(initializationError.failure?.recoverable, false);
      assert.deepEqual(initializationError.failure?.recoveryActions, failure.recoveryActions);
      assert.equal(initializationError.cause, initializationError.failure);
      return true;
    });
    assert.equal(f.readOutcome().status, "rejected", "同代baseline尚未完成时不得解析host");
    assert.equal(f.workers[0].terminations, 1);
    assert.equal(f.workers[0].listenerCount(), 0);
  } finally { await f.cleanup(); }
});
