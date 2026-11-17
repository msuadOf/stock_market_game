import test from "node:test";
import assert from "node:assert/strict";
import { requireTauriContractEnvironment, requireTauriContractBackend } from "./tauri-authority-contract-gate.ts";

test("Desktop产品桥仅允许e2e独立构建显式开关，不允许default分发", { timeout: 10000 }, () => {
  const enabled = { DEV: true, MODE: "e2e", VITE_SESSION_AUTHORITY_CONTRACT: "1" };
  requireTauriContractEnvironment(enabled);
  requireTauriContractEnvironment({ ...enabled, DEV: false });
  for (const changed of [{ MODE: "development" }, { MODE: "production" }, { VITE_SESSION_AUTHORITY_CONTRACT: "0" }]) {
    assert.throws(() => requireTauriContractEnvironment({ ...enabled, ...changed }), /仅允许/);
  }
  assert.throws(() => requireTauriContractEnvironment(undefined), /仅允许/);
});
test("Desktop产品桥必须收到实际host-parity能力，不能默认成功", { timeout: 10000 }, () => {
  requireTauriContractBackend({ verification: true, scenarioVersion: 1, supportedActions: ["initial", "replaced"] });
  for (const value of [null, {}, { verification: false, scenarioVersion: 1, supportedActions: [] }, { verification: true, scenarioVersion: 2, supportedActions: [] }]) {
    assert.throws(() => requireTauriContractBackend(value), /host-parity/);
  }
});
