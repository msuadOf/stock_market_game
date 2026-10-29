import assert from "node:assert/strict";
import test from "node:test";
import { createWorkerHost } from "./worker-host.ts";
import { WasmHostInitializationError } from "./wasm-host-initialization-error.ts";
import { commandDayEndArchiveFixture } from "../app/command-host-test-fixture.ts";
import { DEFAULT_SETUP } from "../config/defaults.ts";
import type { StrictSaveEnvelope } from "../save/schema/root.ts";

const options = { onSpawnFailure: () => {} };

test("WASM 日终启动档在 Worker 创建前以未就绪拓扑错误显式拒绝", { timeout: 10000 }, async () => {
  let allocations = 0;
  const originalWorker = globalThis.Worker;
  globalThis.Worker = class extends EventTarget {
    constructor() { super(); allocations++; }
    terminate() {}
  } as unknown as typeof Worker;
  try {
    const archive = commandDayEndArchiveFixture();
    await assert.rejects(
      createWorkerHost(DEFAULT_SETUP, 1n, { ...options, initialArchive: archive }),
      (error: unknown) => {
        assert.ok(error instanceof WasmHostInitializationError);
        assert.deepEqual(error.failure, {
          code: "WASM_RESTORE_TOPOLOGY_NOT_READY",
          where: "worker-host.initial-archive",
          message: "WASM 日终存档恢复所需的宿主拓扑尚未就绪",
          context: { operation: "restore" },
        });
        return true;
      },
    );
    assert.equal(allocations, 0, "拓扑未就绪拒绝必须发生在 Worker 分配前");
  } finally {
    globalThis.Worker = originalWorker;
  }
});

test("WASM 坏启动档保留严格 parser 错误且零 Worker 分配", { timeout: 10000 }, async () => {
  let allocations = 0;
  const originalWorker = globalThis.Worker;
  globalThis.Worker = class extends EventTarget {
    constructor() { super(); allocations++; }
    terminate() {}
  } as unknown as typeof Worker;
  try {
    const invalid = { ...commandDayEndArchiveFixture(), schema_version: 2 } as unknown as StrictSaveEnvelope;
    await assert.rejects(createWorkerHost(DEFAULT_SETUP, 1n, { ...options, initialArchive: invalid }), /schema_version/);
    assert.equal(allocations, 0, "严格解析失败不能分配 Worker");
  } finally {
    globalThis.Worker = originalWorker;
  }
});

test("WASM 启动档克隆与日终边界拒绝都发生在 Worker 分配前", { timeout: 10000 }, async () => {
  let allocations = 0;
  const originalWorker = globalThis.Worker;
  globalThis.Worker = class extends EventTarget {
    constructor() { super(); allocations++; }
    terminate() {}
  } as unknown as typeof Worker;
  try {
    const cloneFailure = { ...commandDayEndArchiveFixture(), unsupported: () => undefined } as unknown as StrictSaveEnvelope;
    await assert.rejects(createWorkerHost(DEFAULT_SETUP, 1n, { ...options, initialArchive: cloneFailure }), (error: unknown) => {
      assert.ok(error instanceof Error);
      assert.equal(error.name, "DataCloneError");
      return true;
    });
    assert.equal(allocations, 0, "structuredClone 失败不能分配 Worker");

    const intraday = { ...commandDayEndArchiveFixture(), civil_clock: { ...commandDayEndArchiveFixture().civil_clock, settled_through: null } };
    await assert.rejects(createWorkerHost(DEFAULT_SETUP, 1n, { ...options, initialArchive: intraday }), /没有已完成的日终结算/);
    assert.equal(allocations, 0, "日终校验失败不能分配 Worker");
  } finally {
    globalThis.Worker = originalWorker;
  }
});
