import assert from "node:assert/strict";
import test from "node:test";
import { acceptAuthorityContractResult, authorityContractCleanupLease } from "./authority-contract-result.ts";

test("合同换代后失败先接纳真实代际并等待 ingress，再保留主错误", { timeout: 10000 }, async () => {
  const trace: string[] = [];
  await assert.rejects(acceptAuthorityContractResult({ nextGeneration: 2, rows: [], error: "prefix-failure 原始断言" }, 1,
    generation => { trace.push(`generation:${generation}`); }, async () => { trace.push("ingress-ready"); }), /prefix-failure 原始断言/);
  assert.deepEqual(trace, ["generation:2", "ingress-ready"]);
});
test("合同失败与 ingress 清理失败同时保留", { timeout: 10000 }, async () => {
  await assert.rejects(acceptAuthorityContractResult({ nextGeneration: 2, rows: [], error: "合同主错误" }, 1,
    () => {}, async () => { throw new Error("ingress 失败"); }), error => error instanceof AggregateError
      && error.message.includes("合同主错误") && error.errors.some(item => String(item).includes("ingress 失败")));
});

test("成功关档后清理使用最后真实轨迹租约，不再读取归档连接", { timeout: 10000 }, () => {
  assert.equal(authorityContractCleanupLease([{ observer: { lease: "4" } }], undefined,
    () => { throw new Error("归档连接已关闭"); }), 4n);
  assert.equal(authorityContractCleanupLease([], "合同主错误", () => "5"), 5n);
});
