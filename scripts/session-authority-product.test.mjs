import assert from "node:assert/strict";
import test from "node:test";
import { AUTHORITY_SCENARIO } from "./session-authority-trace.mjs";
import { PRODUCT_EXTENSIONS, executeProductContract, requireProductContractCapabilities, validateProductContractEvidence } from "./session-authority-product.mjs";

test("完整产品合同拒绝组件标签、缺少扩展和空成功", { timeout: 10000 }, () => {
  assert.throws(() => requireProductContractCapabilities({ verification: false }, "native"), /验证构建/);
  assert.throws(() => requireProductContractCapabilities({ verification: true, scenarioVersion: 1, supportedActions: [] }, "native"), /未接线/);
  assert.throws(() => validateProductContractEvidence({ execution: "component" }), /真实产品/);
  assert.ok(PRODUCT_EXTENSIONS.includes("prepare-rejected-running-preserved"));
  assert.ok(PRODUCT_EXTENSIONS.includes("terminal-queue-full-prefix-or-gap"));
});

test("平台机制N/A仅允许原生没有WASM worker退出的场景", { timeout: 10000 }, () => {
  const base = { verification: true, scenarioVersion: 1, supportedActions: ["initial", "manual", "without-observer", "slow-observer", "archive-full", "blocked", "old-receipt", "bad-receipt", "duplicate-receipt", "prepare-rejected", "replaced", "old-generation", "new-lease", "old-lease", "prefix-failure", "terminal-repeat", "shutdown", ...PRODUCT_EXTENSIONS.filter(id => id !== "shutdown-unknown-worker-exit-no-reuse")], notApplicable: { "shutdown-unknown-worker-exit-no-reuse": { mechanism: "native-actor", reason: "原生actor没有WASM共享Memory Worker退出机制" } } };
  assert.doesNotThrow(() => requireProductContractCapabilities(base, "native"));
  assert.throws(() => requireProductContractCapabilities(base, "chromium-shared-wasm"), /未接线/);
});

// 这里只测试驱动的错误传播，不产生或冒充任何产品轨迹。
test("驱动同时保留实际动作失败及清理失败", { timeout: 10000 }, async () => {
  const original = new Error("action rejected"), cleanup = new Error("cleanup rejected");
  const capability = { verification: true, scenarioVersion: 1,
    supportedActions: ["initial", "manual", "without-observer", "slow-observer", "archive-full", "blocked", "old-receipt", "bad-receipt", "duplicate-receipt", "prepare-rejected", "replaced", "old-generation", "new-lease", "old-lease", "prefix-failure", "terminal-repeat", "shutdown", ...PRODUCT_EXTENSIONS] };
  await assert.rejects(executeProductContract({ host: "native", capabilities: async () => capability,
    create: async () => { throw original; }, close: async () => { throw cleanup; } }),
    error => error instanceof AggregateError && error.errors[0] === original && error.errors[1] === cleanup);
});

test("单信用机制N/A不能遮盖断线和旧包验收", { timeout: 10000 }, () => {
  const value = { verification: true, scenarioVersion: 1, supportedActions: [...AUTHORITY_SCENARIO.map(row => row.id), ...PRODUCT_EXTENSIONS.filter(id => id !== "disconnect-resync-stale")], notApplicable: {
    "disconnect-resync-stale": { mechanism: "single-credit", reason: "不能以信用数量跳过断线" },
  } };
  assert.throws(() => requireProductContractCapabilities(value, "linux-tauri"), /disconnect-resync-stale/);
});
