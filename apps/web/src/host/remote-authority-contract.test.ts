import test from "node:test";
import assert from "node:assert/strict";
import { assertRemoteContractEnvironment, isContractPublication } from "./remote-authority-contract.ts";

test("Remote产品验证桥仅DEV/e2e同时开启", { timeout: 10000 }, () => {
  assert.throws(() => assertRemoteContractEnvironment(false, "e2e"));
  assert.throws(() => assertRemoteContractEnvironment(true, "production"));
  assert.doesNotThrow(() => assertRemoteContractEnvironment(true, "e2e"));
});
test("暂停真实前端投递时terminal与成功前缀同序排队，Inspector不入业务队列", { timeout: 10000 }, () => {
  assert.equal(isContractPublication({PublisherFrame:{}}), true);
  assert.equal(isContractPublication({PublicationFailure:{}}), true);
  assert.equal(isContractPublication({ObserverTerminal:{}}), true);
  assert.equal(isContractPublication({ObserverEvidence:{}}), false);
});
