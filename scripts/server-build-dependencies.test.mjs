import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { it } from "node:test";

it("production server builds exclude the binding-test TypeScript formatter", () => {
  const tree = execFileSync("cargo", [
    "tree", "--locked", "--offline", "-p", "server", "-e", "normal,build,features",
  ], { cwd: new URL("..", import.meta.url), encoding: "utf8", timeout: 5000 });
  assert.match(tree, /ts-rs v/);
  assert.doesNotMatch(tree, /dprint-plugin-typescript|ts-rs feature "format"/);
});
