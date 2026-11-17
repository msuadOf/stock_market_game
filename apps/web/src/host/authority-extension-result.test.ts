import assert from "node:assert/strict";
import test from "node:test";
import { acceptAuthorityExtensionResult } from "./authority-extension-result.ts";

test("扩展失败仍先接纳真实代际与已消费owner", () => {
  const facts: unknown[] = [];
  assert.throws(() => acceptAuthorityExtensionResult({ nextGeneration: 2, owner: { kind: "consumed", report: {} }, error: "原始失败" }, 1, (generation, owner) => facts.push([generation, owner])), /原始失败/);
  assert.deepEqual(facts, [[2, { kind: "consumed", report: {} }]]);
});
test("未知退出保持unknown，拒绝伪造released标记", () => {
  let owner: unknown;
  acceptAuthorityExtensionResult({ nextGeneration: 1, owner: { kind: "quiescence-unknown", message: "归档未退出" }, result: {} }, 1, (_generation, value) => { owner = value; });
  assert.deepEqual(owner, { kind: "quiescence-unknown", message: "归档未退出" });
  assert.throws(() => acceptAuthorityExtensionResult({ nextGeneration: 1, owner: { kind: "released" } }, 1, () => {}), /owner/);
});
