import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { it } from "node:test";

function dependencyFeatures(packageName, target) {
  return execFileSync("cargo", [
    ...(target === undefined ? [] : ["+nightly-2026-09-05"]),
    "tree", "--locked", "--offline", "-p", packageName, "-e", "features",
    ...(target === undefined ? [] : ["--target", target]),
  ], { cwd: new URL("..", import.meta.url), encoding: "utf8", timeout: 5000 });
}

it("WASM retains ts-rs derives without compiling the native TypeScript formatter", () => {
  const tree = dependencyFeatures("web-wasm", "wasm32-unknown-unknown");
  assert.match(tree, /ts-rs v/);
  assert.doesNotMatch(tree, /dprint-plugin-typescript|ts-rs feature "format"/);
});

it("native bindings keep the formatter used by the committed TypeScript contract", () => {
  const tree = dependencyFeatures("engine");
  assert.match(tree, /ts-rs feature "format"/);
  assert.match(tree, /dprint-plugin-typescript/);
});

it("production server builds exclude the binding-test TypeScript formatter", () => {
  const tree = execFileSync("cargo", [
    "tree", "--locked", "--offline", "-p", "server", "-e", "normal,build,features",
  ], { cwd: new URL("..", import.meta.url), encoding: "utf8", timeout: 5000 });
  assert.match(tree, /ts-rs v/);
  assert.doesNotMatch(tree, /dprint-plugin-typescript|ts-rs feature "format"/);
});
