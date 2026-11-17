import assert from "node:assert/strict";
import { it } from "node:test";
import { requireAuthorityVerificationExports } from "./session-authority-browser.mjs";
it("独立Browser合同拒绝生产包与不完整验证包", { timeout: 10000 }, () => {
  assert.throws(() => requireAuthorityVerificationExports(["market_runner_enable"]), /verification-harness/);
  assert.doesNotThrow(() => requireAuthorityVerificationExports([
    "market_runner_contract_setup", "market_runner_contract_snapshot", "market_runner_contract_fail", "market_runner_contract_queue_full",
  ]));
});
