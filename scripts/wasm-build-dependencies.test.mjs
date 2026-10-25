import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { it } from "node:test";

function dependencyFeatures(packageName, target) {
  // 批 9 A10 收尾：nightly 工具链退役——稳定工具链查询。
  return execFileSync("cargo", [
    "tree", "--locked", "--offline", "-p", packageName, "-e", "features",
    ...(target === undefined ? [] : ["--target", target]),
  ], { cwd: new URL("..", import.meta.url), encoding: "utf8", timeout: 5000 });
}

it("WASM retains ts-rs derives without compiling the native TypeScript formatter", () => {
  const tree = dependencyFeatures("web-wasm", "wasm32-unknown-unknown");
  assert.match(tree, /ts-rs v/);
  assert.doesNotMatch(tree, /dprint-plugin-typescript|ts-rs feature "format"/);
});
