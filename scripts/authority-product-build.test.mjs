import assert from "node:assert/strict";
import test from "node:test";
import { executeAuthorityProductBuild } from "./authority-product-build.mjs";
const input = { root: "/fixture", command: "cargo", args: ["build", "--features", "host-parity"], artifact: "/fixture/app", receiptFile: "/fixture/receipt.json" };
test("产品故障receipt拒绝默认分发与release，不能补签旧前端", { timeout: 10000 }, async () => {
  let invoked = false;
  const build = async () => { invoked = true; };
  await assert.rejects(executeAuthorityProductBuild({ ...input, args: ["build", "--release", "--features", "host-parity"] }, build), /debug_assertions/);
  await assert.rejects(executeAuthorityProductBuild({ ...input, args: ["build"] }, build), /host-parity/);
  await assert.rejects(executeAuthorityProductBuild(input, build), /frontendDist/);
  assert.equal(invoked, false, "不完整身份不能启动真实构建");
});
