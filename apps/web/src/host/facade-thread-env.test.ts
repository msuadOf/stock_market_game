import assert from "node:assert/strict";
import test from "node:test";

import * as envModule from "./engine-clock-env.ts";

// wasm 线程门面 bootstrap（R4a 阶段 2，设计 §12-14）：`env` 模块必须导出
// 槽协议全部 import 名，否则 wasm 实例化期直接 link error（import 解析
// 发生在实例化，不在调用）。
//
// 当前宿主尚未装配 supervisor/子 Worker/bootstrap，槽协议路径不可达；
// 函数体必须是**显式失败**（铁律二：不静默 no-op 伪装成功）。

const FACADE_IMPORT_NAMES = [
  "facade_futex_wait",
  "facade_futex_notify",
  "facade_spawn_notify",
  "facade_worker_exit_notify",
  "facade_terminate_child",
] as const;

test("env 模块导出全部槽协议 import 名（否则 wasm 实例化 link error）", () => {
  for (const name of FACADE_IMPORT_NAMES) {
    assert.equal(
      typeof (envModule as Readonly<Record<string, unknown>>)[name],
      "function",
      `env.${name} 缺失：wasm 实例化会因缺少 import 而失败`,
    );
  }
});

test("槽协议环境函数在未装配的宿主上显式失败，不静默成功", () => {
  assert.throws(
    () => envModule.facade_futex_wait(0, 0, -1n),
    /facade_thread_env|共享内存/,
    "futex_wait 必须在宿主未装配 时显式抛出",
  );
  assert.throws(() => envModule.facade_futex_notify(0), /facade_thread_env|共享内存/);
  assert.throws(() => envModule.facade_spawn_notify(0, 0, 0, 0), /facade_thread_env|共享内存/);
  assert.throws(() => envModule.facade_worker_exit_notify(0, 0, 0), /facade_thread_env|共享内存/);
  assert.throws(() => envModule.facade_terminate_child(0), /facade_thread_env|共享内存/);
});
